//! Windows command implementations (TD-0002 §5.2). The CLI services one target per process,
//! so DISM calls run inline on the main thread instead of through a `TargetActor`.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use dpro_cli_fmt::{exit::ExitCode, messages::Messages, plan::render_plan, table};
use dpro_core::{
    cancel::CancelToken,
    id::TargetId,
    plan::{OpDescriptor, OsBuild, PLAN_SCHEMA, Plan, PlanDoc, PlanSummary},
};
use dpro_engine::{
    discover::{self, WindowsSystem},
    dism_backend::DismBackend,
    identity,
    job::{self, MonotonicClock, RunConfig},
    journal::{self, Begin, JournalWriter},
    ops::{self, FeatureEntry, PackageEntry},
    paths,
    registry::{OfflineRegistry, OnlineRegistry},
};
use dpro_win::{
    dism::{DismApi, LogLevel, Session},
    privilege, system,
};

use crate::{
    args::{Cli, Command, TargetSel},
    error::CliError,
    json::{FeatureJson, FeaturesDoc, PackageJson, PackagesDoc, TargetJson, TargetsDoc, state_name},
    out::{self, Confirm, Decision},
};

pub(crate) fn run(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    match &cli.command {
        Command::TargetsList => targets(cli, m),
        Command::Health { .. } | Command::HealthRestore | Command::RebootReasons => {
            Err(CliError::NotImplemented("health commands"))
        }
        Command::Help | Command::Version | Command::JournalShow(_) => Ok(ExitCode::Ok),
        dism_command => {
            if !privilege::is_elevated()? {
                return Err(CliError::NeedsElevation);
            }
            let target = Resolved::new(&cli.global.target)?;
            // Read identity before DISM opens the image: offline hives can't be loaded twice.
            let (target_id, os_build) = target.identity();
            let api = DismApi::initialize(LogLevel::Errors, None, None)?;
            let session = target.open(&api)?;
            let ctx = Ctx { cli, m, target: &target, target_id, os_build, session: &session };
            match dism_command {
                Command::PackagesList => ctx.packages(),
                Command::FeaturesList => ctx.features(),
                Command::FeaturesSet { enable, names } => {
                    let current = ctx.feature_entries()?;
                    ctx.apply(ops::plan_features(target_id, os_build, &current, names, *enable)?)
                }
                Command::PackagesRemove(ids) => {
                    let current: Vec<PackageEntry> = session
                        .packages()?
                        .into_iter()
                        .map(|p| PackageEntry { identity: p.name.to_string_lossy().into_owned() })
                        .collect();
                    ctx.apply(ops::plan_package_removal(target_id, os_build, &current, ids)?)
                }
                Command::JournalUndo(path) => ctx.undo(path),
                _ => Ok(ExitCode::Ok),
            }
        }
    }
}

fn targets(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    let found = discover::discover(&WindowsSystem);
    let rows: Vec<TargetJson> = found
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
        .collect();
    if cli.global.json {
        out::print_json(&TargetsDoc { schema: "targets.v1", targets: rows })?;
    } else {
        let mut t = table::Table::new(&[(m.text("col-target"), false), (m.text("col-kind"), false), (m.text("col-path"), false)]);
        for r in rows {
            t.push(vec![r.selector, r.kind.to_owned(), r.windows_dir]);
        }
        print!("{}", table::render(&t, out::width()));
    }
    Ok(ExitCode::Ok)
}

/// A `--target` resolved to concrete paths.
struct Resolved {
    /// Where the journal lives (`<volume>\ProgramData\DismPro`).
    volume_root: PathBuf,
    /// The image root DISM opens (`DISM_ONLINE_IMAGE` for the running OS).
    image_root: Option<PathBuf>,
    windows_dir: PathBuf,
}

impl Resolved {
    fn new(sel: &TargetSel) -> Result<Self, CliError> {
        Ok(match sel {
            TargetSel::Online => {
                let windows_dir = system::windows_directory()?;
                let volume_root = windows_dir.parent().map_or_else(|| PathBuf::from("C:\\"), Path::to_path_buf);
                Self { volume_root, image_root: None, windows_dir }
            }
            TargetSel::Root(root) | TargetSel::Image(root) => {
                let windows_dir = root.join("Windows");
                if system::probe(&windows_dir.join("System32").join("config").join("SYSTEM")).is_none() {
                    return Err(CliError::Usage(format!("no Windows installation at {}", root.display())));
                }
                Self { volume_root: root.clone(), image_root: Some(root.clone()), windows_dir }
            }
        })
    }

    /// `(TargetId, OsBuild)`; zeros when the registry can't be read (CAVEATS.md §9).
    fn identity(&self) -> (TargetId, OsBuild) {
        let unknown = OsBuild { major: 0, minor: 0, build: 0, revision: 0 };
        let pair = match &self.image_root {
            None => OnlineRegistry::open().ok().map(|r| (identity::target_id(&r), identity::os_build(&r))),
            Some(_) => {
                OfflineRegistry::load(&self.windows_dir).ok().map(|r| (identity::target_id(&r), identity::os_build(&r)))
            }
        };
        let (id, build) = pair.unwrap_or((None, None));
        (id.unwrap_or(TargetId([0; 16])), build.unwrap_or(unknown))
    }

    fn open<'api>(&self, api: &'api DismApi) -> Result<Session<'api>, CliError> {
        Ok(match &self.image_root {
            None => Session::open_online(api)?,
            Some(root) => Session::open_offline(api, root, None)?,
        })
    }
}

struct Ctx<'a, 's, 'api> {
    cli: &'a Cli,
    m: &'a Messages,
    target: &'a Resolved,
    target_id: TargetId,
    os_build: OsBuild,
    session: &'s Session<'api>,
}

impl Ctx<'_, '_, '_> {
    fn packages(&self) -> Result<ExitCode, CliError> {
        let rows: Vec<PackageJson> = self
            .session
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
            .collect();
        if self.cli.global.json {
            out::print_json(&PackagesDoc { schema: "packages.v1", packages: rows })?;
        } else {
            let m = self.m;
            let mut t = table::Table::new(&[
                (m.text("col-package"), false),
                (m.text("col-state"), false),
                (m.text("col-installed"), false),
            ]);
            for r in rows {
                t.push(vec![r.identity, m.text(&format!("state-{}", r.state)), r.installed]);
            }
            print!("{}", table::render(&t, out::width()));
        }
        Ok(ExitCode::Ok)
    }

    fn feature_entries(&self) -> Result<Vec<FeatureEntry>, CliError> {
        Ok(self
            .session
            .features()?
            .into_iter()
            .map(|f| FeatureEntry {
                name: f.name.to_string_lossy().into_owned(),
                // Installed or install-pending counts as enabled.
                enabled: matches!(f.state, 4 | 5),
            })
            .collect())
    }

    fn features(&self) -> Result<ExitCode, CliError> {
        let rows: Vec<FeatureJson> = self
            .session
            .features()?
            .into_iter()
            .map(|f| FeatureJson {
                name: f.name.to_string_lossy().into_owned(),
                state: state_name(f.state),
                enabled: matches!(f.state, 4 | 5),
            })
            .collect();
        if self.cli.global.json {
            out::print_json(&FeaturesDoc { schema: "features.v1", features: rows })?;
        } else {
            let m = self.m;
            let mut t = table::Table::new(&[(m.text("col-feature"), false), (m.text("col-state"), false)]);
            for r in rows {
                t.push(vec![r.name, m.text(&format!("state-{}", r.state))]);
            }
            print!("{}", table::render(&t, out::width()));
        }
        Ok(ExitCode::Ok)
    }

    /// Undo a journaled job: the inverse changes become a new plan, previewed and journaled
    /// like any other (TD-0001 §5.2). Refused if the OS build changed since the job.
    fn undo(&self, path: &Path) -> Result<ExitCode, CliError> {
        let rec = journal::read(&fs::read(path)?)?;
        if rec.begin.os_build.build != self.os_build.build {
            return Err(CliError::Journal(format!(
                "the job was made on build {}, the target is now build {}; undo is disabled",
                rec.begin.os_build.build, self.os_build.build
            )));
        }
        let changes = journal::undo_changes(&rec, &[], paths::now());
        let doc = PlanDoc {
            schema: PLAN_SCHEMA.to_owned(),
            target: rec.begin.target,
            os_build: rec.begin.os_build,
            op: OpDescriptor {
                id: "journal.undo".into(),
                title_key: "op-journal-undo".into(),
                category: "journal".into(),
                cancellable: false,
            },
            changes,
            summary: PlanSummary::default(),
        };
        self.apply(Plan::from_doc(doc)?)
    }

    /// Print the plan, confirm, apply with a journal, report (TD-0002 §4, §5.2).
    fn apply(&self, plan: Plan) -> Result<ExitCode, CliError> {
        let g = &self.cli.global;
        let doc = plan.to_doc()?;
        let summary = plan.summary();
        if g.json {
            out::print_json(&doc)?;
        } else {
            print!("{}", render_plan(&doc, self.m, out::width()));
        }
        let confirm = Confirm {
            dry_run: g.dry_run,
            yes: g.yes,
            allow_irreversible: g.allow_irreversible,
            interactive: !g.json && out::stdin_is_console(),
        };
        match out::decide(confirm, summary.changes, summary.irreversible, out::ask, self.m) {
            Decision::Stop(ExitCode::ConfirmationRequired) => {
                eprintln!("{}", self.m.text("cli-confirmation-required"));
                return Ok(ExitCode::ConfirmationRequired);
            }
            Decision::Stop(code) => return Ok(code),
            Decision::Apply => {}
        }

        let job = paths::new_job_id();
        let journal_path = paths::journal_file(&self.target.volume_root, job);
        if let Some(dir) = journal_path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = File::options().write(true).create_new(true).open(&journal_path)?;
        let begin = Begin { job, target: self.target_id, os_build: self.os_build, created: paths::now() };
        let writer = JournalWriter::create(file, &begin)?;
        let mut backend =
            DismBackend { session: self.session, sources: g.sources.clone(), limit_access: g.limit_access };
        let (result, _) =
            job::run(&doc.changes, &mut backend, writer, &CancelToken::new(), &MonotonicClock::new(), RunConfig::default())?;

        let rec = journal::read(&fs::read(&journal_path)?)?;
        let job_doc = out::job_doc(&rec, &journal_path);
        if g.json {
            out::print_json(&job_doc)?;
        } else {
            out::print_job(&job_doc, self.m);
        }
        Ok(out::job_exit(result.status, result.applied, result.failed))
    }
}
