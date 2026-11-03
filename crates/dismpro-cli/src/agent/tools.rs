//! The broker's side of the agent tools (TD-0013 §5.3, §15). Every call is checked again here,
//! against the policy as it is at that moment: the front's check only spares a UAC prompt.
//!
//! DISM calls and offline hive reads run on one DISM thread, in order: `DismInitialize` works
//! once per process, DISM's handles stay on the thread that made them, and an offline image's
//! hives can't be loaded twice. Everything else runs on the calling front's connection thread.

use std::{
    collections::VecDeque,
    convert::Infallible,
    fmt::Write as _,
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use dpro_broker::{
    Handler,
    wire::{Caller, Outcome},
};
use dpro_cli_fmt::{messages::Messages, units::format_bytes};
use dpro_core::{
    health::{ImageHealth, LogFile},
    id::JobId,
    plan::{OsBuild, Plan, PlanDoc, PlanSummary},
    units::Bytes,
};
use dpro_engine::{
    actor::TargetActor,
    health::{self, logs},
    journal, paths,
};
use dpro_mcp::{
    catalog::{self, Tier, Tool},
    gate,
    policy::Policy,
    shape::{self, ListArgs, MAX_RESULT_BYTES, Row},
    tool::CallError,
};
use dpro_win::{
    dism::{Call, DismApi, LogLevel},
    pipe, security, system,
};
use serde_json::{Map, Value, json};

use super::{MAX_POLICY_BYTES, explain, parse_policy, policy_path, rows::change_rows};
use crate::{
    args::{self, TargetSel},
    error::CliError,
    json::hresult,
    out,
    run::{self, Resolved},
};

/// Plans kept in memory; the oldest goes first (TD-0013 §15).
const MAX_PLANS: usize = 64;
/// How much of the end of CBS.log `health_diagnose` reads. CBS.log grows to tens of MB before
/// Windows archives it; the recent part is what explains a recent failure, and reading less
/// keeps the call inside the read-tool budget (TD-0013 §7, to be measured on G1).
const DIAGNOSE_WINDOW: u64 = 8 << 20;
/// Rows of a list a result carries when the tool has no paging of its own.
const PREVIEW_ROWS: u32 = 20;

const NEXT_PLAN: &str = "Show the user this plan, including anything permanent. plan_get pages \
    through every change; plan_apply asks a human to approve it.";
const NEXT_REPAIRABLE: &str =
    "health_diagnose shows what's wrong; health_repair_plan plans a repair.";
const NEXT_NOT_REPAIRABLE: &str = "health_diagnose shows what's wrong. A repair needs a source the \
    store can't provide: health_repair_plan with sources (install.wim of the same build).";
const NOTE_UNEXPLAINED: &str = "No log line explains this state: a quick check only reads the \
    store's corruption flag. A full scan (dismpro-cli health scan, several minutes) logs the cause.";
const NOTE_HOST_DISM_LOG: &str = "An offline image's servicing stack logs to this computer's DISM \
    log, which every DISM session here shares: a line may be about another image or the running \
    Windows. The image's own Windows\\Logs\\CBS\\CBS.log, if any, is from when it last ran.";
const NOT_AVAILABLE: &str = "not_available: this version can't apply plans for agents. Applying \
    arrives with DISM Pro's approval card; until then the user can make the change with \
    dismpro-cli.";
const DISM_GONE: &str = "broker_unavailable: the broker's DISM thread stopped. Ask the user to \
    restart the MCP server.";

/// The tools the broker runs, with the state they share across fronts.
#[derive(Debug)]
pub(crate) struct BrokerTools {
    catalog: Vec<Tool>,
    /// The running Windows' volume: the policy lives there whatever the target.
    volume_root: PathBuf,
    /// `None` once the broker shuts down.
    dism: Mutex<Option<TargetActor<Dism>>>,
    plans: Mutex<VecDeque<StoredPlan>>,
    m: Arc<Messages>,
}

/// The DISM thread's state. DISM starts on the first call that needs it, so a broker that
/// only lists targets or journals never loads it.
#[derive(Debug, Default)]
struct Dism {
    api: Option<DismApi>,
}

impl Dism {
    fn api(&mut self) -> Result<&DismApi, CliError> {
        let api = match &mut self.api {
            Some(api) => api,
            empty @ None => empty.insert(DismApi::initialize(LogLevel::Errors, None, None)?),
        };
        Ok(api)
    }
}

#[derive(Debug)]
struct StoredPlan {
    id: String,
    fingerprint: String,
    target: String,
    doc: PlanDoc,
}

/// A target as a call names it: `shown` for results, `sel` to resolve.
#[derive(Debug)]
struct Target {
    shown: String,
    sel: TargetSel,
}

impl BrokerTools {
    /// # Errors
    /// The Windows folder can't be found, or the DISM thread can't start.
    pub(crate) fn start() -> Result<Self, CliError> {
        let windows = system::windows_directory()?;
        let volume_root = windows
            .parent()
            .map_or_else(|| PathBuf::from("C:\\"), Path::to_path_buf);
        let (actor, _ready) = TargetActor::spawn("dism", || Ok::<_, Infallible>(Dism::default()))?;
        Ok(Self {
            catalog: catalog::tools(),
            volume_root,
            dism: Mutex::new(Some(actor)),
            plans: Mutex::new(VecDeque::new()),
            m: Arc::new(Messages::en_us()),
        })
    }

    /// Lets queued DISM calls finish, then shuts DISM down.
    pub(crate) fn shutdown(&self) {
        let actor = lock(&self.dism).take();
        drop(actor);
    }

    fn run(&self, name: &str, args: &Value) -> Result<Value, CallError> {
        let policy = self.policy()?;
        let allowed = gate::check(&self.catalog, Some(&policy), name, args)?;
        let shown = args
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("online");
        let target = Target {
            shown: shown.to_owned(),
            sel: args::target(shown).map_err(|e| CallError::Invalid(e.0))?,
        };
        let mut result = match allowed.tool.def.name {
            "targets_list" => Ok(targets(&policy)),
            "system_overview" => self.overview(target),
            "health_check" => self.health_check(target),
            "health_diagnose" => self.diagnose(&target, &list_args(args)?),
            "updates_list" => self.updates(target, list_args(args)?),
            "journal_list" => self.journal(&target, &list_args(args)?),
            "undo_plan" => self.undo(target, args, &policy),
            "health_repair_plan" => self.repair(target, args, &policy),
            "plan_get" => self.plan_get(args),
            "plan_apply" => Err(CallError::Failed(NOT_AVAILABLE.to_owned())),
            "explain_error" => {
                let text = args.get("code").and_then(Value::as_str).unwrap_or("");
                let code = explain::parse_code(text).map_err(CallError::Invalid)?;
                Ok(explain::explain(code, system::error_text, &self.m))
            }
            other => Err(CallError::UnknownTool(other.to_owned())),
        }?;
        if policy.redact_user_paths {
            shape::redact_user_paths(&mut result);
        }
        Ok(result)
    }

    /// The policy as it is now, read through a handle whose security was checked (TD-0013 §15).
    fn policy(&self) -> Result<Policy, CallError> {
        let path = policy_path(&self.volume_root);
        match security::read_admin_only_file(&path, MAX_POLICY_BYTES) {
            Ok(None) => Ok(Policy::default()),
            Ok(Some(bytes)) => parse_policy(&bytes).map_err(|e| {
                CallError::Failed(format!(
                    "policy_invalid: {}: {e}. Every tool is refused until the user fixes or \
                     removes the file.",
                    path.display()
                ))
            }),
            Err(e) => Err(CallError::Failed(format!("policy_untrusted: {e}"))),
        }
    }

    /// Runs `f` on the DISM thread and waits for it.
    fn on_dism<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Dism) -> R + Send + 'static,
    ) -> Result<R, CallError> {
        let gone = || CallError::Failed(DISM_GONE.to_owned());
        let reply = {
            let actor = lock(&self.dism);
            actor
                .as_ref()
                .ok_or_else(gone)?
                .submit(f)
                .map_err(|_| gone())?
        };
        reply.recv().map_err(|_| gone())
    }

    fn overview(&self, target: Target) -> Result<Value, CallError> {
        let m = Arc::clone(&self.m);
        self.on_dism(move |dism: &mut Dism| -> Result<Value, CallError> {
            let fail = |e: CliError| failed(&e, &m);
            let t = Resolved::new(&target.sel).map_err(fail)?;
            // Registry first: DISM loads an offline image's hives itself.
            let facts = t.facts();
            let (reasons, dirty) = t.reboot_reasons().map_err(fail)?;
            let (free, total) = system::disk_space(&t.volume_root).map_err(|e| fail(e.into()))?;
            let api = dism.api().map_err(fail)?;
            let session = t.open(api).map_err(fail)?;
            let code = session
                .check_health(false, quiet())
                .map_err(|e| fail(e.into()))?;
            let state = health::image_health(code);
            let mut v = json!({
                "target": target.shown,
                "windows_dir": t.windows_dir.display().to_string(),
                "build": build_text(facts.os_build),
                "edition": facts.edition,
                "health": state,
                "restart": {
                    "pending": !reasons.is_empty(),
                    "reasons": run::reboot_messages(&reasons, &m),
                },
                "space": {
                    "free_bytes": free,
                    "free": format_bytes(Bytes(free), &m),
                    "total_bytes": total,
                    "total": format_bytes(Bytes(total), &m),
                },
            });
            if dirty {
                v["restart"]["hives_incomplete"] = json!(true);
            }
            if let Some(next) = health_next(state) {
                v["next"] = json!(next);
            }
            Ok(v)
        })?
    }

    fn health_check(&self, target: Target) -> Result<Value, CallError> {
        let m = Arc::clone(&self.m);
        self.on_dism(move |dism: &mut Dism| -> Result<Value, CallError> {
            let fail = |e: CliError| failed(&e, &m);
            let t = Resolved::new(&target.sel).map_err(fail)?;
            let api = dism.api().map_err(fail)?;
            let session = t.open(api).map_err(fail)?;
            let (state, log, rows) =
                run::check_health(&session, &t, false, quiet(), &m).map_err(fail)?;
            let preview = preview(&rows)?;
            let mut v = json!({
                "target": target.shown,
                "health": state,
                "log": log.display().to_string(),
                "findings": preview.items,
            });
            if preview.next_cursor.is_some() {
                v["more_findings"] = json!(preview.matched);
            }
            if rows.is_empty() && state != ImageHealth::Healthy {
                v["note"] = json!(NOTE_UNEXPLAINED);
            }
            if let Some(next) = health_next(state) {
                v["next"] = json!(next);
            }
            Ok(v)
        })?
    }

    /// Findings in the end of the log a health check of the target writes to
    /// ([`run::health_log`]), newest first.
    fn diagnose(&self, target: &Target, list: &ListArgs) -> Result<Value, CallError> {
        let fail = |e: &CliError| failed(e, &self.m);
        let t = Resolved::new(&target.sel).map_err(|e| fail(&e))?;
        let (log, file) = run::health_log(&t).map_err(|e| fail(&e))?;
        let read = recent_findings(&log, file, DIAGNOSE_WINDOW)
            .map_err(|e| fail(&CliError::io_at(&log, &e)))?;
        let Some((mut findings, from_line)) = read else {
            return Ok(json!({
                "target": target.shown,
                "log": Value::Null,
                "items": [],
                "matched": 0,
                "note": format!("{} doesn't exist: nothing has been serviced here yet.", log.display()),
            }));
        };
        findings.reverse();
        let rows = run::finding_rows(&findings, &self.m);
        let mut v = page(&rows, list)?;
        v.insert("target".into(), json!(target.shown));
        v.insert("log".into(), json!(log.display().to_string()));
        v.insert("from_line".into(), json!(from_line));
        if file == LogFile::Dism {
            v.insert("note".into(), json!(NOTE_HOST_DISM_LOG));
        }
        Ok(Value::Object(v))
    }

    fn updates(&self, target: Target, list: ListArgs) -> Result<Value, CallError> {
        let m = Arc::clone(&self.m);
        self.on_dism(move |dism: &mut Dism| -> Result<Value, CallError> {
            let fail = |e: CliError| failed(&e, &m);
            let t = Resolved::new(&target.sel).map_err(fail)?;
            let api = dism.api().map_err(fail)?;
            let session = t.open(api).map_err(fail)?;
            let rows = run::package_rows(&session).map_err(fail)?;
            let mut v = page(&rows, &list)?;
            v.insert("target".into(), json!(target.shown));
            Ok(Value::Object(v))
        })?
    }

    /// Journaled jobs, newest first.
    fn journal(&self, target: &Target, list: &ListArgs) -> Result<Value, CallError> {
        let t = Resolved::new(&target.sel).map_err(|e| failed(&e, &self.m))?;
        let dir = paths::journal_dir(&t.volume_root);
        let entries =
            journal::list(&dir).map_err(|e| failed(&CliError::io_at(&dir, &e), &self.m))?;
        let mut rows = out::journal_rows(&entries);
        rows.reverse();
        let mut v = page(&rows, list)?;
        v.insert("target".into(), json!(target.shown));
        Ok(Value::Object(v))
    }

    fn undo(&self, target: Target, args: &Value, policy: &Policy) -> Result<Value, CallError> {
        let text = args.get("job_id").and_then(Value::as_str).unwrap_or("");
        let job: JobId = text.parse().map_err(|_| {
            CallError::Invalid(format!(
                "{text:?} isn't a job id. journal_list shows the jobs."
            ))
        })?;
        let m = Arc::clone(&self.m);
        let shown = target.shown.clone();
        let made = self.on_dism(move |dism: &mut Dism| -> Result<Made, CallError> {
            let fail = |e: CliError| failed(&e, &m);
            let t = Resolved::new(&target.sel).map_err(fail)?;
            let path = paths::journal_file(&t.volume_root, job);
            if !path.is_file() {
                return Err(CallError::Invalid(format!(
                    "{} has no job {job}. journal_list shows its jobs.",
                    target.shown
                )));
            }
            let identity = t.identity();
            let features = || {
                let session = t.open(dism.api()?)?;
                run::feature_entries(&session)
            };
            let (plan, _) = run::undo_plan(&t, identity, &path, features, &m).map_err(fail)?;
            owned(&plan)
        })??;
        self.store(shown, made, policy, Map::new())
    }

    fn repair(&self, target: Target, args: &Value, policy: &Policy) -> Result<Value, CallError> {
        let limit_access = args
            .get("limit_access")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let sources: Vec<PathBuf> = args
            .get("sources")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let m = Arc::clone(&self.m);
        let shown = target.shown.clone();
        let made = self.on_dism(move |_: &mut Dism| -> Result<Made, CallError> {
            let t = Resolved::new(&target.sel).map_err(|e| failed(&e, &m))?;
            let (target_id, os_build) = t.identity();
            let restore = health::RestoreArgs {
                sources,
                limit_access,
            };
            let plan = health::plan_restore(target_id, os_build, &restore)
                .map_err(|e| CallError::Invalid(e.to_string()))?;
            owned(&plan)
        })??;
        let mut extra = Map::new();
        extra.insert("contacts_windows_update".into(), json!(!limit_access));
        self.store(shown, made, policy, extra)
    }

    /// Keeps a plan for `plan_get` and `plan_apply` and describes it (TD-0013 §5.3).
    fn store(
        &self,
        target: String,
        (doc, summary): Made,
        policy: &Policy,
        extra: Map<String, Value>,
    ) -> Result<Value, CallError> {
        if summary.changes > u64::from(policy.max_changes_per_plan) {
            return Err(CallError::Failed(format!(
                "policy_forbids: the plan has {} changes; the agent policy allows {} per plan. \
                 The user can raise max_changes_per_plan in DISM Pro: Settings, Agent access.",
                summary.changes, policy.max_changes_per_plan
            )));
        }
        let bytes = serde_json::to_vec(&doc).map_err(|e| CallError::Failed(e.to_string()))?;
        let digest = pipe::sha256(&bytes).map_err(|e| CallError::Failed(e.to_string()))?;
        let fingerprint = format!("sha256:{}", hex(&digest));
        let id = format!("pl_{}", paths::new_job_id());
        let rows = change_rows(&doc.changes);
        let preview = preview(&rows)?;
        let required = if summary.irreversible > 0 {
            Tier::ApplyWithApproval
        } else {
            Tier::ApplyReversible
        };
        let mut about = json!({
            "op": doc.op.id,
            "changes": summary.changes,
            "irreversible": summary.irreversible,
            "reboot": summary.reboot_required,
            "required_tier": required.name(),
        });
        if let Value::Object(fields) = &mut about {
            fields.extend(extra);
        }
        let mut v = json!({
            "plan_id": id,
            "fingerprint": fingerprint,
            "target": target,
            "summary": about,
            "changes": preview.items,
            "next": NEXT_PLAN,
        });
        if preview.next_cursor.is_some() {
            v["more_changes"] = json!(true);
        }
        let mut plans = lock(&self.plans);
        if plans.len() >= MAX_PLANS {
            plans.pop_front();
        }
        plans.push_back(StoredPlan {
            id,
            fingerprint,
            target,
            doc,
        });
        Ok(v)
    }

    fn plan_get(&self, args: &Value) -> Result<Value, CallError> {
        let id = args.get("plan_id").and_then(Value::as_str).unwrap_or("");
        let list = ListArgs {
            limit: args
                .get("limit")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            cursor: args
                .get("cursor")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..ListArgs::default()
        };
        let plans = lock(&self.plans);
        let plan = plans.iter().find(|p| p.id == id).ok_or_else(|| {
            CallError::Failed(format!(
                "plan_unknown: no plan {id:?}. Plans last until the broker exits (15 minutes \
                 after the last call) and only the newest {MAX_PLANS} are kept; make it again."
            ))
        })?;
        let mut v = page(&change_rows(&plan.doc.changes), &list)?;
        v.insert("plan_id".into(), json!(plan.id));
        v.insert("fingerprint".into(), json!(plan.fingerprint));
        v.insert("target".into(), json!(plan.target));
        Ok(Value::Object(v))
    }
}

impl Handler for BrokerTools {
    fn call(&self, tool: &str, args: Value, _caller: &Caller) -> Outcome {
        match self.run(tool, &args) {
            Ok(v) => Outcome::Ok(v),
            Err(CallError::UnknownTool(t)) => Outcome::UnknownTool(t),
            Err(CallError::Invalid(s)) => Outcome::Invalid(s),
            Err(CallError::Failed(s)) => Outcome::Failed(s),
        }
    }
}

/// Every target, with whether the policy lets agents name it.
fn targets(policy: &Policy) -> Value {
    let rows: Vec<Value> = run::target_rows()
        .into_iter()
        .map(|r| {
            let allowed = gate::parse_target(&r.selector)
                .is_ok_and(|t| policy.check_target(t, &r.selector).is_ok());
            json!({
                "selector": r.selector,
                "kind": r.kind,
                "windows_dir": r.windows_dir,
                "allowed": allowed,
            })
        })
        .collect();
    json!({ "targets": rows })
}

/// The list arguments of a call, without the tool's own (`target`).
fn list_args(args: &Value) -> Result<ListArgs, CallError> {
    let mut rest = args.as_object().cloned().unwrap_or_default();
    rest.remove("target");
    serde_json::from_value(Value::Object(rest)).map_err(|e| CallError::Invalid(e.to_string()))
}

/// One page of `rows` as a JSON object, ready for the tool's own fields.
fn page<R: Row>(rows: &[R], list: &ListArgs) -> Result<Map<String, Value>, CallError> {
    let page = shape::page(rows, list, MAX_RESULT_BYTES - 1024)
        .map_err(|e| CallError::Invalid(e.to_string()))?;
    match serde_json::to_value(page) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(CallError::Failed(
            "a page didn't serialize to an object".into(),
        )),
        Err(e) => Err(CallError::Failed(e.to_string())),
    }
}

/// The first rows of a list that a result carries inline.
fn preview<R: Row>(rows: &[R]) -> Result<shape::Page, CallError> {
    let first = ListArgs {
        limit: Some(PREVIEW_ROWS),
        ..ListArgs::default()
    };
    shape::page(rows, &first, MAX_RESULT_BYTES / 2).map_err(|e| CallError::Failed(e.to_string()))
}

/// A plan as the broker keeps it: owned strings, so it can leave the DISM thread.
type Made = (PlanDoc, PlanSummary);

/// A [`Plan`] as [`Made`].
fn owned(plan: &Plan) -> Result<Made, CallError> {
    let doc = plan
        .to_doc()
        .map_err(|e| CallError::Failed(e.to_string()))?;
    Ok((doc, plan.summary()))
}

/// The findings in the last `window` bytes of a log, with line numbers counted from the start of
/// the file, and the first line read. `None` when the log doesn't exist.
fn recent_findings(
    log: &Path,
    file: LogFile,
    window: u64,
) -> io::Result<Option<(Vec<dpro_core::health::Finding>, u64)>> {
    let f = match File::open(log) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let start = f.metadata()?.len().saturating_sub(window);
    let reader = BufReader::with_capacity(64 << 10, f);
    logs::scan_after(reader, start, file).map(Some)
}

/// How an agent reads a `CliError`: the same words the CLI prints. A usage error is the
/// agent's to fix (`Invalid`); anything else failed.
fn failed(e: &CliError, m: &Messages) -> CallError {
    let detail = e.detail();
    let text = m.get(e.diagnosis(), &[("message", &detail), ("what", &detail)]);
    let text = match e.os() {
        Some(os) => format!("{text} ({} {})", os.api, hresult(os.code)),
        None => text,
    };
    match e {
        CliError::Usage(_) => CallError::Invalid(text),
        _ => CallError::Failed(text),
    }
}

fn health_next(state: ImageHealth) -> Option<&'static str> {
    match state {
        ImageHealth::Healthy => None,
        ImageHealth::Repairable => Some(NEXT_REPAIRABLE),
        ImageHealth::NonRepairable => Some(NEXT_NOT_REPAIRABLE),
    }
}

/// `10.0.26100.4652`, or `null` when the registry couldn't be read.
fn build_text(b: OsBuild) -> Value {
    if b.build == 0 {
        Value::Null
    } else {
        json!(format!(
            "{}.{}.{}.{}",
            b.major, b.minor, b.build, b.revision
        ))
    }
}

/// A DISM call no one cancels or watches: agent calls are short, and the front shows no
/// progress yet.
fn quiet() -> Call<'static> {
    Call {
        cancel: None,
        progress: None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}"); // ignore: writing to a String can't fail
            s
        })
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic aborts the process (release profile), so a poisoned lock only happens in tests;
    // the data is still consistent there.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_args_drop_the_target() {
        let args = json!({ "target": "online", "limit": 5, "filter": "kb" });
        let list = list_args(&args).unwrap();
        assert_eq!(list.limit, Some(5));
        assert_eq!(list.filter.as_deref(), Some("kb"));
        assert!(list_args(&json!({ "target": "online", "nope": 1 })).is_err());
    }

    #[test]
    fn fingerprints_are_lowercase_hex() {
        assert_eq!(hex(&[0x00, 0xab, 0x10]), "00ab10");
    }

    #[test]
    fn builds_print_dotted_or_null() {
        let b = OsBuild {
            major: 10,
            minor: 0,
            build: 26100,
            revision: 4652,
        };
        assert_eq!(build_text(b), json!("10.0.26100.4652"));
        assert_eq!(build_text(OsBuild { build: 0, ..b }), Value::Null);
    }
}
