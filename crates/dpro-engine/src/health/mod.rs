//! Health & Repair (TD-0006): log findings, reboot reasons, and the restore plan.

pub mod logs;
pub mod reboot;

use std::path::PathBuf;

use dpro_core::{
    Error,
    health::ImageHealth,
    id::TargetId,
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, Plan, PlanBuilder, Reversibility},
};

/// `Native.api` of a component-store repair.
pub const RESTORE_API: &str = "DismRestoreImageHealth";

/// `DismImageHealthState` value → [`ImageHealth`] (verify the numbering with ffi-check).
#[must_use]
pub fn image_health(state: i32) -> ImageHealth {
    match state {
        0 => ImageHealth::Healthy,
        1 => ImageHealth::Repairable,
        _ => ImageHealth::NonRepairable,
    }
}

/// Arguments of a restore, encoded in `Native.args` as lines: `limit_access=<bool>` then one
/// source path per line (paths can't contain newlines on Windows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreArgs {
    pub sources: Vec<PathBuf>,
    pub limit_access: bool,
}

impl RestoreArgs {
    #[must_use]
    pub fn encode(&self) -> String {
        let mut s = format!("limit_access={}", self.limit_access);
        for p in &self.sources {
            s.push('\n');
            s.push_str(&p.to_string_lossy());
        }
        s
    }

    #[must_use]
    pub fn decode(s: &str) -> Option<Self> {
        let mut lines = s.lines();
        let limit_access = match lines.next()? {
            "limit_access=true" => true,
            "limit_access=false" => false,
            _ => return None,
        };
        Some(Self {
            sources: lines.map(PathBuf::from).collect(),
            limit_access,
        })
    }
}

/// `health restore` (TD-0006 G2): one irreversible `Native` change. Without `limit_access`
/// Windows Update may be contacted; front ends say so (`health-restore-wu-note`).
///
/// # Errors
/// Arena errors only.
pub fn plan_restore(
    target: TargetId,
    os_build: OsBuild,
    args: &RestoreArgs,
) -> Result<Plan, Error> {
    let op = OpDescriptor {
        id: "health.restore".into(),
        title_key: "op-health-restore".into(),
        category: "health".into(),
        cancellable: false,
    };
    let mut b = PlanBuilder::new(target, os_build, op);
    let api = b.atom(RESTORE_API)?;
    let encoded = b.atom(&args.encode())?;
    let reason = b.atom("rev-component-store")?;
    b.push(Change {
        kind: ChangeKind::Native { api, args: encoded },
        reversibility: Reversibility::Irreversible { reason },
        reboot_required: true,
    })?;
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_args_round_trip() {
        let a = RestoreArgs {
            sources: vec![
                PathBuf::from(r"E:\sources\install.wim"),
                PathBuf::from(r"F:\x"),
            ],
            limit_access: true,
        };
        assert_eq!(RestoreArgs::decode(&a.encode()), Some(a));
        assert_eq!(RestoreArgs::decode("nope"), None);
    }

    #[test]
    fn restore_plan_is_one_permanent_change() {
        let args = RestoreArgs {
            sources: vec![],
            limit_access: false,
        };
        let plan = plan_restore(
            TargetId([0; 16]),
            OsBuild {
                major: 10,
                minor: 0,
                build: 1,
                revision: 0,
            },
            &args,
        )
        .unwrap();
        assert_eq!(plan.summary().changes, 1);
        assert_eq!(plan.summary().irreversible, 1);
    }
}
