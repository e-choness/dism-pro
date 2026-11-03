//! Windows command implementations (TD-0002 §5.2). The CLI services one target per process,
//! so DISM calls run inline on the main thread instead of through a `TargetActor`.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use dpro_cli_fmt::{
    exit::ExitCode, messages::Messages, plan::render_plan, table, units::format_utc,
};
use dpro_core::{
    OsError,
    cancel::CancelToken,
    health::LogFile,
    id::{JobId, TargetId},
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, PLAN_SCHEMA, Plan, PlanDoc, PlanSummary},
    progress::Progress,
};
use dpro_engine::{
    discover::{self, WindowsSystem},
    dism_backend::DismBackend,
    health::{
        self, logs,
        reboot::{self as reboot_mod, RealFiles},
    },
    identity,
    job::{self, Applied, Backend, MonotonicClock, RunConfig},
    journal::{self, Begin, JobStatus, JournalWriter},
    ops::{self, FeatureEntry, PackageEntry},
    paths,
    registry::{OfflineRegistry, OnlineRegistry},
};
use dpro_win::{
    console,
    dism::{Call, DismApi, LogLevel, Session},
    event::Event,
    privilege, security, system,
};

use crate::{
    args::{Cli, Command, JournalRef, TargetSel},
    error::CliError,
    json::{
        FeatureJson, FeaturesDoc, FindingJson, HealthDoc, JournalsDoc, PackageJson, PackagesDoc,
        RebootDoc, TargetJson, TargetsDoc, hresult, state_name,
    },
    out::{self, Confirm, Decision, ProgressLine},
};

pub(crate) fn run(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    match &cli.command {
        Command::TargetsList => targets(cli, m),
        Command::RebootReasons => reboot_reasons(cli, m),
        Command::Purge => purge(cli, m),
        Command::JournalList => journal_list(cli, m),
        Command::JournalShow(JournalRef::Id(job)) => {
            require_elevation()?; // the journal folder is admin-only
            let target = Resolved::new(&cli.global.target)?;
            crate::journal_show(cli, m, &paths::journal_file(&target.volume_root, *job))
        }
        Command::Mcp { toolsets } => crate::agent::front::run(toolsets.as_deref()),
        Command::AgentBroker { user_sid } => crate::agent::broker::run(user_sid),
        Command::Help | Command::Version | Command::JournalShow(JournalRef::File(_)) => {
            Ok(ExitCode::Ok)
        }
        dism_command => {
            require_elevation()?;
            let target = Resolved::new(&cli.global.target)?;
            // Read identity before DISM opens the image: offline hives can't be loaded twice.
            let (target_id, os_build) = target.identity();
            let cancel = CancelToken::new();
            let cancel_event = console::cancel_on_ctrl_c(&cancel)?;
            let api = DismApi::initialize(LogLevel::Errors, None, None)?;
            let mut ctx = Ctx {
                cli,
                m,
                target: &target,
                target_id,
                os_build,
                session: target.open(&api)?,
                cancel,
                cancel_event,
            };
            match dism_command {
                Command::PackagesList => ctx.packages(),
                Command::FeaturesList => ctx.features(),
                Command::FeaturesSet { enable, names } => {
                    let current = ctx.feature_entries()?;
                    let plan = ops::plan_features(target_id, os_build, &current, names, *enable)?;
                    ctx.apply(&plan, None)
                }
                Command::PackagesRemove(ids) => {
                    let current = ctx.package_entries()?;
                    let plan = ops::plan_package_removal(target_id, os_build, &current, ids)?;
                    ctx.apply(&plan, None)
                }
                Command::JournalUndo(job) => ctx.undo(job),
                Command::Health { scan } => ctx.health(*scan),
                Command::HealthRestore => {
                    let args = health::RestoreArgs {
                        sources: cli.global.sources.clone(),
                        limit_access: cli.global.limit_access,
                    };
                    if !args.limit_access && !cli.global.json {
                        eprintln!("{}", m.text("health-restore-wu-note"));
                    }
                    ctx.apply(&health::plan_restore(target_id, os_build, &args)?, None)
                }
                _ => Ok(ExitCode::Ok),
            }
        }
    }
}

fn require_elevation() -> Result<(), CliError> {
    if privilege::is_elevated()? {
        Ok(())
    } else {
        Err(CliError::NeedsElevation)
    }
}

fn targets(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    let rows = target_rows();
    if cli.global.json {
        out::print_json(&TargetsDoc {
            schema: "targets.v1",
            targets: rows,
        })?;
    } else {
        let mut t = table::Table::new(&[
            (m.text("col-target"), false),
            (m.text("col-kind"), false),
            (m.text("col-path"), false),
        ]);
        for r in rows {
            t.push(vec![r.selector, r.kind.to_owned(), r.windows_dir]);
        }
        print!("{}", table::render(&t, out::width()));
    }
    Ok(ExitCode::Ok)
}

/// Every target on this machine, as `targets list` and the agent's `targets_list` show them.
pub(crate) fn target_rows() -> Vec<TargetJson> {
    discover::discover(&WindowsSystem)
        .iter()
        .map(|t| match t {
            dpro_core::target::Target::Online { windows_dir } => TargetJson {
                kind: t.kind(),
                selector: "online".into(),
                windows_dir: windows_dir.display().to_string(),
            },
            dpro_core::target::Target::OfflineInstall { root, windows_dir } => TargetJson {
                kind: t.kind(),
                selector: root.display().to_string(),
                windows_dir: root.join(windows_dir).display().to_string(),
            },
            dpro_core::target::Target::MountedImage { mount_dir, .. } => TargetJson {
                kind: t.kind(),
                selector: format!("image:{}", mount_dir.display()),
                windows_dir: mount_dir.join("Windows").display().to_string(),
            },
        })
        .collect()
}

/// `reboot-reasons` (TD-0006 F-16). Online needs no elevation; offline hives do.
fn reboot_reasons(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    let target = Resolved::new(&cli.global.target)?;
    if target.image_root.is_some() {
        require_elevation()?;
    }
    let (reasons, dirty) = target.reboot_reasons()?;
    if dirty {
        eprintln!("{}", m.text("reboot-dirty-hives"));
    }
    let messages = reboot_messages(&reasons, m);
    if cli.global.json {
        out::print_json(&RebootDoc {
            schema: "reboot-reasons.v1",
            pending: !reasons.is_empty(),
            reasons,
            messages,
        })?;
    } else if messages.is_empty() {
        println!("{}", m.text("reboot-none"));
    } else {
        for line in messages {
            println!("- {line}");
        }
    }
    Ok(ExitCode::Ok)
}

/// `journal list`: every journaled job on the target, oldest first.
fn journal_list(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    require_elevation()?; // the journal folder is admin-only
    let target = Resolved::new(&cli.global.target)?;
    let dir = paths::journal_dir(&target.volume_root);
    let entries = journal::list(&dir).map_err(|e| CliError::io_at(&dir, &e))?;
    let rows = out::journal_rows(&entries);
    if cli.global.json {
        out::print_json(&JournalsDoc {
            schema: "journals.v1",
            jobs: rows,
        })?;
    } else if rows.is_empty() {
        println!("{}", m.text("journal-none"));
    } else {
        let mut t = table::Table::new(&[
            (m.text("col-job"), false),
            (m.text("col-created"), false),
            (m.text("col-operation"), false),
            (m.text("col-status"), false),
            (m.text("col-applied"), true),
            (m.text("col-undo"), false),
        ]);
        for (e, r) in entries.iter().zip(rows) {
            t.push(vec![
                r.job,
                format_utc(e.begin.created, m),
                m.text(&format!("op-{}", r.op.replace('.', "-"))),
                m.text(&format!("job-status-{}", r.status)),
                r.applied.to_string(),
                r.undone_by
                    .map(|by| m.get("job-undone-by", &[("job", &by)]))
                    .unwrap_or_default(),
            ]);
        }
        print!("{}", table::render(&t, out::width()));
    }
    Ok(ExitCode::Ok)
}

/// `purge` (roadmap 2.8): delete every file DISM Pro created on the target's volume. The plan is
/// irreversible, so it needs `--yes --allow-irreversible` or typing the count.
fn purge(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    require_elevation()?;
    let target = Resolved::new(&cli.global.target)?;
    let (target_id, os_build) = target.identity();
    let plan = dpro_engine::purge::plan_purge(target_id, os_build, &target.volume_root)
        .map_err(|e| CliError::Failed(e.to_string()))?;
    let doc = plan.to_doc()?;
    let g = &cli.global;
    if g.json {
        out::print_json(&doc)?;
    } else {
        print!("{}", render_plan(&doc, m, out::width()));
    }
    if let Some(code) = confirm(g, &plan, m) {
        return Ok(code);
    }
    let cancel = CancelToken::new();
    console::cancel_on_ctrl_c(&cancel)?;
    let begin = Begin {
        job: paths::new_job_id(),
        target: target_id,
        os_build,
        created: paths::now(),
        op: doc.op.id.clone(),
        undoes: None,
    };
    // In-memory journal: a journal file would recreate the state being purged.
    let writer = JournalWriter::create(Vec::new(), &begin)?;
    let (result, _) = job::run(
        &doc.changes,
        &mut dpro_engine::purge::FsBackend,
        writer,
        &cancel,
        &MonotonicClock::new(),
        RunConfig::default(),
    )?;
    for dir in dpro_engine::purge::owned_dirs(&target.volume_root) {
        dpro_engine::purge::remove_empty_dirs(&dir);
    }
    if !g.json {
        println!(
            "{} {}",
            m.text(&format!(
                "job-status-{}",
                out::status_name(Some(result.status))
            )),
            m.get(
                "cli-applied",
                &[
                    ("applied", &result.applied),
                    ("count", &plan.summary().changes)
                ]
            )
        );
    }
    Ok(out::job_exit(result.status, result.applied, result.failed))
}

/// Asks for (or checks flags for) confirmation. `Some(code)`: stop with that exit code.
fn confirm(g: &crate::args::Global, plan: &Plan, m: &Messages) -> Option<ExitCode> {
    let s = plan.summary();
    match out::decide(Confirm::new(g), s.changes, s.irreversible, out::ask, m) {
        Decision::Apply => None,
        Decision::Stop(ExitCode::ConfirmationRequired) => {
            eprintln!("{}", m.text("cli-confirmation-required"));
            Some(ExitCode::ConfirmationRequired)
        }
        Decision::Stop(code) => Some(code),
    }
}

/// A `--target` resolved to concrete paths.
pub(crate) struct Resolved {
    /// Where the journal lives (`<volume>\ProgramData\DismPro`).
    pub(crate) volume_root: PathBuf,
    /// The image root DISM opens (`DISM_ONLINE_IMAGE` for the running OS).
    pub(crate) image_root: Option<PathBuf>,
    pub(crate) windows_dir: PathBuf,
}

/// What a target's registry says about it.
pub(crate) struct Facts {
    pub(crate) target_id: TargetId,
    pub(crate) os_build: OsBuild,
    pub(crate) edition: Option<String>,
}

impl Resolved {
    pub(crate) fn new(sel: &TargetSel) -> Result<Self, CliError> {
        Ok(match sel {
            TargetSel::Online => {
                let windows_dir = system::windows_directory()?;
                let volume_root = windows_dir
                    .parent()
                    .map_or_else(|| PathBuf::from("C:\\"), Path::to_path_buf);
                Self {
                    volume_root,
                    image_root: None,
                    windows_dir,
                }
            }
            TargetSel::Root(root) | TargetSel::Image(root) => {
                let windows_dir = root.join("Windows");
                if system::probe(&windows_dir.join("System32").join("config").join("SYSTEM"))
                    .is_none()
                {
                    return Err(CliError::Usage(format!(
                        "no Windows installation at {}",
                        root.display()
                    )));
                }
                Self {
                    volume_root: root.clone(),
                    image_root: Some(root.clone()),
                    windows_dir,
                }
            }
        })
    }

    /// `(TargetId, OsBuild)`; zeros when the registry can't be read (CAVEATS.md §9).
    pub(crate) fn identity(&self) -> (TargetId, OsBuild) {
        let facts = self.facts();
        (facts.target_id, facts.os_build)
    }

    /// Identity and edition from one read of the registry. Read it before DISM opens an
    /// offline image: its hives can't be loaded twice.
    pub(crate) fn facts(&self) -> Facts {
        fn read(reg: &impl dpro_engine::registry::RegistryView) -> Facts {
            Facts {
                target_id: identity::target_id(reg).unwrap_or(UNKNOWN_TARGET),
                os_build: identity::os_build(reg).unwrap_or(UNKNOWN_BUILD),
                edition: identity::edition(reg),
            }
        }
        let facts = match &self.image_root {
            None => OnlineRegistry::open().ok().map(|r| read(&r)),
            Some(_) => OfflineRegistry::load(&self.windows_dir)
                .ok()
                .map(|r| read(&r)),
        };
        facts.unwrap_or(Facts {
            target_id: UNKNOWN_TARGET,
            os_build: UNKNOWN_BUILD,
            edition: None,
        })
    }

    /// Why a restart is pending (TD-0006 F-16), and whether offline hives were dirty (their
    /// last write didn't finish, so recent changes may be missing). Offline needs elevation.
    pub(crate) fn reboot_reasons(
        &self,
    ) -> Result<(Vec<dpro_core::health::RebootReason>, bool), CliError> {
        Ok(if self.image_root.is_none() {
            let reasons =
                reboot_mod::reboot_reasons(&OnlineRegistry::open()?, &RealFiles, &self.windows_dir);
            (reasons, false)
        } else {
            let hives = OfflineRegistry::load(&self.windows_dir)?;
            let reasons = reboot_mod::reboot_reasons(&hives, &RealFiles, &self.windows_dir);
            (reasons, hives.is_dirty())
        })
    }

    pub(crate) fn open<'api>(&self, api: &'api DismApi) -> Result<Session<'api>, CliError> {
        Ok(match &self.image_root {
            None => Session::open_online(api)?,
            Some(root) => Session::open_offline(api, root, None)?,
        })
    }
}

/// The reasons in the user's words; a pending file rename says how many.
pub(crate) fn reboot_messages(
    reasons: &[dpro_core::health::RebootReason],
    m: &Messages,
) -> Vec<String> {
    reasons
        .iter()
        .map(|r| match r {
            dpro_core::health::RebootReason::FileRename { count } => {
                m.get(r.message_id(), &[("count", count)])
            }
            _ => m.text(r.message_id()),
        })
        .collect()
}

/// The identity of a target whose registry couldn't be read.
pub(crate) const UNKNOWN_TARGET: TargetId = TargetId([0; 16]);
const UNKNOWN_BUILD: OsBuild = OsBuild {
    major: 0,
    minor: 0,
    build: 0,
    revision: 0,
};

struct Ctx<'a, 'api> {
    cli: &'a Cli,
    m: &'a Messages,
    target: &'a Resolved,
    target_id: TargetId,
    os_build: OsBuild,
    session: Session<'api>,
    /// Cancelled by Ctrl+C; jobs stop at the next change.
    cancel: CancelToken,
    /// Set by Ctrl+C; stops read-only DISM calls that accept it.
    cancel_event: &'static Event,
}

impl Ctx<'_, '_> {
    fn packages(&self) -> Result<ExitCode, CliError> {
        let m = self.m;
        if self.cli.global.json {
            out::print_json(&PackagesDoc {
                schema: "packages.v1",
                packages: package_rows(&self.session)?,
            })?;
        } else {
            let mut t = table::Table::new(&[
                (m.text("col-package"), false),
                (m.text("col-state"), false),
                (m.text("col-installed"), false),
            ]);
            for p in self.session.packages()? {
                let i = p.install_time;
                t.push(vec![
                    p.name.to_string_lossy().into_owned(),
                    state_text(m, p.state),
                    format!("{:04}-{:02}-{:02}", i.wYear, i.wMonth, i.wDay),
                ]);
            }
            print!("{}", table::render(&t, out::width()));
        }
        Ok(ExitCode::Ok)
    }

    fn package_entries(&self) -> Result<Vec<PackageEntry>, CliError> {
        Ok(self
            .session
            .packages()?
            .into_iter()
            .map(|p| PackageEntry {
                identity: p.name.to_string_lossy().into_owned(),
            })
            .collect())
    }

    fn feature_entries(&self) -> Result<Vec<FeatureEntry>, CliError> {
        feature_entries(&self.session)
    }

    fn features(&self) -> Result<ExitCode, CliError> {
        let features = self.session.features()?;
        let m = self.m;
        if self.cli.global.json {
            let rows = features
                .into_iter()
                .map(|f| FeatureJson {
                    name: f.name.to_string_lossy().into_owned(),
                    state: state_name(f.state),
                    enabled: is_enabled(f.state),
                })
                .collect();
            out::print_json(&FeaturesDoc {
                schema: "features.v1",
                features: rows,
            })?;
        } else {
            let mut t =
                table::Table::new(&[(m.text("col-feature"), false), (m.text("col-state"), false)]);
            for f in features {
                t.push(vec![
                    f.name.to_string_lossy().into_owned(),
                    state_text(m, f.state),
                ]);
            }
            print!("{}", table::render(&t, out::width()));
        }
        Ok(ExitCode::Ok)
    }

    /// `health check|scan` (TD-0006 §5.2): read-only, so no plan, and Ctrl+C cancels it.
    /// Findings come from the part of the log written during the check ([`health_log`]).
    fn health(&self, scan: bool) -> Result<ExitCode, CliError> {
        let m = self.m;
        let progress = ProgressLine::new(self.cli.global.json, m);
        let sink = |p: Progress| progress.report(p);
        let call = Call {
            cancel: Some(self.cancel_event),
            progress: Some(&sink),
        };
        let checked = check_health(&self.session, self.target, scan, call, m);
        progress.finish();
        let (state, log, rows) = checked?;
        let state_id = match state {
            dpro_core::health::ImageHealth::Healthy => "health-state-healthy",
            dpro_core::health::ImageHealth::Repairable => "health-state-repairable",
            dpro_core::health::ImageHealth::NonRepairable => "health-state-non-repairable",
        };
        let unexplained = state != dpro_core::health::ImageHealth::Healthy && rows.is_empty();
        if self.cli.global.json {
            out::print_json(&HealthDoc {
                schema: "health.v1",
                state,
                scanned: scan,
                log: log.display().to_string(),
                findings: rows,
            })?;
        } else {
            println!("{}", m.text(state_id));
            if unexplained {
                // A quick check reads only the store's flag; a scan looks at every component.
                let id = if scan {
                    "health-unexplained"
                } else {
                    "health-unexplained-check"
                };
                println!("{}", m.get(id, &[("log", &log.display().to_string())]));
            }
            if !rows.is_empty() {
                let mut t = table::Table::new(&[
                    (m.text("col-severity"), false),
                    (m.text("col-finding"), false),
                    (m.text("col-lines"), true),
                ]);
                for r in rows {
                    let severity = match r.severity {
                        dpro_core::health::Severity::Info => "severity-info",
                        dpro_core::health::Severity::Warning => "severity-warning",
                        dpro_core::health::Severity::Error => "severity-error",
                    };
                    let message = match r.code {
                        Some(code) => format!("{} ({code})", r.message),
                        None => r.message,
                    };
                    t.push(vec![
                        m.text(severity),
                        message,
                        format!("{}-{}", r.first_line, r.last_line),
                    ]);
                }
                println!();
                print!("{}", table::render(&t, out::width()));
            }
            if state == dpro_core::health::ImageHealth::NonRepairable {
                println!("{}", m.text("health-next-source"));
            }
        }
        Ok(ExitCode::Ok)
    }

    /// Undo a journaled job: the inverse changes become a new plan, previewed and journaled
    /// like any other (TD-0001 §5.2). Refused for another installation, another build, or a
    /// job that was already undone. For an interrupted job, feature changes without an
    /// outcome are probed against the image.
    fn undo(&mut self, job_ref: &JournalRef) -> Result<ExitCode, CliError> {
        let path = match job_ref {
            JournalRef::Id(job) => paths::journal_file(&self.target.volume_root, *job),
            JournalRef::File(path) => path.clone(),
        };
        let (plan, job) = undo_plan(
            self.target,
            (self.target_id, self.os_build),
            &path,
            || self.feature_entries(),
            self.m,
        )?;
        self.apply(&plan, Some(job))
    }

    /// Print the plan, confirm, apply with a journal, report (TD-0002 §4, §5.2).
    fn apply(&mut self, plan: &Plan, undoes: Option<JobId>) -> Result<ExitCode, CliError> {
        let g = &self.cli.global;
        let m = self.m;
        let doc = plan.to_doc()?;
        if g.json {
            out::print_json(&doc)?;
        } else {
            print!("{}", render_plan(&doc, m, out::width()));
        }
        if let Some(code) = confirm(g, plan, m) {
            return Ok(code);
        }

        // Admin-only state folder (TD-0001 §8): only SYSTEM and Administrators may write the
        // journals this process replays.
        let root = &self.target.volume_root;
        security::ensure_admin_only_dir(&paths::state_dir(root))?;
        let dir = paths::journal_dir(root);
        fs::create_dir_all(&dir).map_err(|e| CliError::io_at(&dir, &e))?;
        let job = paths::new_job_id();
        let journal_path = paths::journal_file(root, job);
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&journal_path)
            .map_err(|e| CliError::io_at(&journal_path, &e))?;
        let begin = Begin {
            job,
            target: self.target_id,
            os_build: self.os_build,
            created: paths::now(),
            op: doc.op.id.clone(),
            undoes,
        };
        let writer = JournalWriter::create(file, &begin)?;

        let progress = ProgressLine::new(g.json, m);
        let sink = |p: Progress| progress.report(p);
        let mut backend = Stepping {
            inner: DismBackend {
                session: &mut self.session,
                sources: g.sources.clone(),
                limit_access: g.limit_access,
                progress: Some(&sink),
                errors: Vec::new(),
            },
            line: &progress,
            total: doc.changes.len(),
            started: 0,
        };
        let ran = job::run(
            &doc.changes,
            &mut backend,
            writer,
            &self.cancel,
            &MonotonicClock::new(),
            RunConfig::default(),
        );
        progress.finish();
        let (summary, _) = ran?;
        for e in &backend.inner.errors {
            eprintln!("{}", m.get("cli-error", &[("message", e)]));
        }
        if summary.status == JobStatus::Cancelled {
            eprintln!("{}", m.text("cli-cancelled"));
        }

        let bytes = fs::read(&journal_path).map_err(|e| CliError::io_at(&journal_path, &e))?;
        let rec = journal::read(&bytes)?;
        if g.json {
            out::print_json(&out::job_doc(&rec, &journal_path))?;
        } else {
            out::print_job(&rec, &journal_path, m);
        }
        Ok(out::job_exit(
            summary.status,
            summary.applied,
            summary.failed,
        ))
    }
}

/// The packages of an open session, as `packages list --json` and `updates_list` show them.
pub(crate) fn package_rows(session: &Session<'_>) -> Result<Vec<PackageJson>, CliError> {
    Ok(session
        .packages()?
        .into_iter()
        .map(|p| {
            let t = p.install_time;
            PackageJson {
                identity: p.name.to_string_lossy().into_owned(),
                state: state_name(p.state),
                release_type: p.release_type,
                installed: format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
                ),
            }
        })
        .collect())
}

/// The log a health check of `target` explains itself in (TD-0006 §7): the running Windows's
/// CBS.log, or for an offline image the servicing host's dism.log. DISM runs the image's
/// servicing stack in its own process, which logs to the log DISM was initialized with (the
/// default, since DISM Pro passes none), and the image's own CBS.log stays untouched.
pub(crate) fn health_log(target: &Resolved) -> Result<(PathBuf, LogFile), CliError> {
    Ok(if target.image_root.is_none() {
        let cbs = target.windows_dir.join("Logs").join("CBS").join("CBS.log");
        (cbs, LogFile::Cbs)
    } else {
        let host = system::windows_directory()?;
        (
            host.join("Logs").join("DISM").join("dism.log"),
            LogFile::Dism,
        )
    })
}

/// A health check or scan (TD-0006 §5.2), the log it explains itself in, and its findings:
/// that log's lines written during the call.
pub(crate) fn check_health(
    session: &Session<'_>,
    target: &Resolved,
    scan: bool,
    call: Call<'_>,
    m: &Messages,
) -> Result<(dpro_core::health::ImageHealth, PathBuf, Vec<FindingJson>), CliError> {
    let (log, file) = health_log(target)?;
    let start = fs::metadata(&log).map_or(0, |md| md.len());
    let state = health::image_health(session.check_health(scan, call)?);
    let findings = match File::open(&log) {
        Ok(f) => {
            // A rotated log is shorter than `start`: read it from the beginning.
            let len = f.metadata().map_or(0, |md| md.len());
            let skip = if len >= start { start } else { 0 };
            let reader = std::io::BufReader::with_capacity(64 << 10, f);
            logs::scan_after(reader, skip, file)?.0
        }
        Err(_) => Vec::new(),
    };
    Ok((state, log, finding_rows(&findings, m)))
}

pub(crate) fn finding_rows(
    findings: &[dpro_core::health::Finding],
    m: &Messages,
) -> Vec<FindingJson> {
    findings
        .iter()
        .map(|f| FindingJson {
            severity: f.severity,
            diagnosis: f.diagnosis,
            message: m.text(f.diagnosis),
            code: f.code.map(|c| hresult(c.code)),
            log: f.source.file,
            first_line: f.source.first_line,
            last_line: f.source.last_line,
        })
        .collect()
}

/// The plan that undoes a journaled job: its inverse changes, previewed and journaled like any
/// other plan (TD-0001 §5.2). Refused for another installation, another build, or a job that
/// was already undone. For an interrupted job, feature changes without an outcome are probed
/// against the image, which is what `features` reads. Returns the plan and the job it undoes.
pub(crate) fn undo_plan(
    target: &Resolved,
    (target_id, os_build): (TargetId, OsBuild),
    journal_file: &Path,
    features: impl FnOnce() -> Result<Vec<FeatureEntry>, CliError>,
    m: &Messages,
) -> Result<(Plan, JobId), CliError> {
    let bytes = fs::read(journal_file).map_err(|e| CliError::io_at(journal_file, &e))?;
    let rec = journal::read(&bytes)?;
    let job = rec.begin.job;
    let known = |t: TargetId| t != UNKNOWN_TARGET;
    if known(rec.begin.target) && known(target_id) && rec.begin.target != target_id {
        return Err(CliError::Failed(
            m.get("journal-other-target", &[("job", &job)]),
        ));
    }
    if rec.begin.os_build.build != os_build.build {
        return Err(CliError::Failed(m.get(
            "journal-other-build",
            &[
                ("job", &job),
                ("then", &rec.begin.os_build.build),
                ("now", &os_build.build),
            ],
        )));
    }
    let dir = paths::journal_dir(&target.volume_root);
    let entries = journal::list(&dir).map_err(|e| CliError::io_at(&dir, &e))?;
    if let Some(by) = journal::undone_by(&entries, job) {
        return Err(CliError::Failed(
            m.get("journal-already-undone", &[("job", &job), ("by", &by)]),
        ));
    }
    let report = journal::analyze(&rec);
    let probed = if report.status.is_none() && !report.without_outcome.is_empty() {
        ops::probe_features(&rec, &report.without_outcome, &features()?)
    } else {
        Vec::new()
    };
    let doc = PlanDoc {
        schema: PLAN_SCHEMA.to_owned(),
        target: target_id,
        os_build,
        op: OpDescriptor {
            id: "journal.undo".into(),
            title_key: "op-journal-undo".into(),
            category: "journal".into(),
            cancellable: false,
        },
        changes: journal::undo_changes(&rec, &probed, paths::now()),
        summary: PlanSummary::default(),
    };
    Ok((Plan::from_doc(doc)?, job))
}

/// The features of an open session, as the planners and the undo probe take them.
pub(crate) fn feature_entries(session: &Session<'_>) -> Result<Vec<FeatureEntry>, CliError> {
    Ok(session
        .features()?
        .into_iter()
        .map(|f| FeatureEntry {
            name: f.name.to_string_lossy().into_owned(),
            enabled: is_enabled(f.state),
        })
        .collect())
}

/// Installed or install-pending counts as enabled (`DismStateInstalled`,
/// `DismStateInstallPending`).
fn is_enabled(state: i32) -> bool {
    matches!(state, 4 | 5)
}

/// Localized `DismPackageFeatureState`; unknown values show their number.
fn state_text(m: &Messages, state: i32) -> String {
    m.get(
        &format!("state-{}", state_name(state)),
        &[("value", &state)],
    )
}

/// Tells the progress line which change of a multi-change job is running.
struct Stepping<'p, 'm, B> {
    inner: B,
    line: &'p ProgressLine<'m>,
    total: usize,
    started: usize,
}

impl<B: Backend> Backend for Stepping<'_, '_, B> {
    fn capture_inverse(
        &mut self,
        change: &Change<String>,
    ) -> Result<Option<ChangeKind<String>>, OsError> {
        self.inner.capture_inverse(change)
    }

    fn apply(&mut self, change: &Change<String>) -> Result<Applied, OsError> {
        self.started += 1;
        if self.total > 1 {
            self.line.step(self.started, self.total);
        }
        self.inner.apply(change)
    }
}
