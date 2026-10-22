//! Operations that plan DISM changes from the target's current state (TD-0001 §5.1, TD-0002 §4).
//! Pure: the caller lists features/packages through the target actor and passes them in.

use dpro_core::{
    Error,
    id::TargetId,
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, Plan, PlanBuilder, Reversibility},
};

/// A feature as the planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntry {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub identity: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error(transparent)]
    Core(#[from] Error),
    #[error("unknown {what}: {name}")]
    Unknown { what: &'static str, name: String },
}

/// `features enable|disable <names>`. Names match case-insensitively (DISM does too);
/// features already in the requested state are skipped. Each change is undone by the
/// opposite change while the payload stays on disk (disable keeps it, TD-0001 §3).
///
/// # Errors
/// [`PlanError::Unknown`] for a name the target doesn't have.
pub fn plan_features(
    target: TargetId,
    os_build: OsBuild,
    current: &[FeatureEntry],
    names: &[String],
    enable: bool,
) -> Result<Plan, PlanError> {
    let id = if enable { "features.enable" } else { "features.disable" };
    let mut b = PlanBuilder::new(target, os_build, descriptor(id, "op-features"));
    for wanted in names {
        let feature = current
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| PlanError::Unknown { what: "feature", name: wanted.clone() })?;
        if feature.enabled == enable {
            continue;
        }
        let name = b.atom(&feature.name)?;
        b.push(Change {
            kind: ChangeKind::FeatureState { name, enable, source: None },
            reversibility: Reversibility::ReversibleVia {
                inverse: Box::new(ChangeKind::FeatureState { name, enable: !enable, source: None }),
            },
            // DISM reports the real need at apply time; plan conservatively (UX: Reboot badge).
            reboot_required: true,
        })?;
    }
    Ok(b.finish())
}

/// `packages remove <identities>`. Irreversible: re-adding needs the original package file.
///
/// # Errors
/// [`PlanError::Unknown`] for an identity the target doesn't have.
pub fn plan_package_removal(
    target: TargetId,
    os_build: OsBuild,
    current: &[PackageEntry],
    identities: &[String],
) -> Result<Plan, PlanError> {
    let mut b = PlanBuilder::new(target, os_build, descriptor("packages.remove", "op-packages-remove"));
    let reason = b.atom("rev-package-remove")?;
    for wanted in identities {
        let package = current
            .iter()
            .find(|p| p.identity.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| PlanError::Unknown { what: "package", name: wanted.clone() })?;
        let identity = b.atom(&package.identity)?;
        b.push(Change {
            kind: ChangeKind::PackageRemove { identity },
            reversibility: Reversibility::Irreversible { reason },
            reboot_required: true,
        })?;
    }
    Ok(b.finish())
}

fn descriptor(id: &str, title_key: &str) -> OpDescriptor {
    OpDescriptor { id: id.to_owned(), title_key: title_key.to_owned(), category: "dism".to_owned(), cancellable: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: OsBuild = OsBuild { major: 10, minor: 0, build: 26100, revision: 0 };

    fn features() -> Vec<FeatureEntry> {
        vec![
            FeatureEntry { name: "SMB1Protocol".into(), enabled: true },
            FeatureEntry { name: "TelnetClient".into(), enabled: false },
        ]
    }

    #[test]
    fn disables_only_what_is_enabled() {
        let names = vec!["smb1protocol".to_owned(), "TelnetClient".to_owned()];
        let plan = plan_features(TargetId([0; 16]), BUILD, &features(), &names, false).unwrap();
        let doc = plan.to_doc().unwrap();
        assert_eq!(doc.changes.len(), 1);
        assert_eq!(
            doc.changes[0].kind,
            ChangeKind::FeatureState { name: "SMB1Protocol".into(), enable: false, source: None }
        );
        assert!(matches!(doc.changes[0].reversibility, Reversibility::ReversibleVia { .. }));
    }

    #[test]
    fn unknown_names_fail_the_whole_plan() {
        let err = plan_features(TargetId([0; 16]), BUILD, &features(), &["Nope".to_owned()], true).unwrap_err();
        assert!(matches!(err, PlanError::Unknown { what: "feature", .. }));
    }

    #[test]
    fn package_removal_is_irreversible() {
        let current = vec![PackageEntry { identity: "Pkg~31bf3856ad364e35~amd64~~1.0".into() }];
        let plan = plan_package_removal(TargetId([0; 16]), BUILD, &current, &["pkg~31bf3856ad364e35~amd64~~1.0".into()]).unwrap();
        assert_eq!(plan.summary().irreversible, 1);
    }
}
