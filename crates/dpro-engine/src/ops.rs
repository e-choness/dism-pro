//! Operations that plan DISM changes from the target's current state (TD-0001 §5.1, TD-0002 §4).
//! Pure: the caller lists features/packages through the target actor and passes them in.

use dpro_core::{
    Error,
    id::TargetId,
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, Plan, PlanBuilder, Reversibility},
};

use crate::journal::{Intent, Record, Recovered};

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
    let (id, title) = if enable {
        ("features.enable", "op-features-enable")
    } else {
        ("features.disable", "op-features-disable")
    };
    let mut b = PlanBuilder::new(target, os_build, descriptor(id, title));
    let mut planned: Vec<&str> = Vec::new();
    for wanted in names {
        let feature = current
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| PlanError::Unknown {
                what: "feature",
                name: wanted.clone(),
            })?;
        if feature.enabled == enable || planned.contains(&feature.name.as_str()) {
            continue;
        }
        planned.push(&feature.name);
        let name = b.atom(&feature.name)?;
        b.push(Change {
            kind: ChangeKind::FeatureState {
                name,
                enable,
                source: None,
            },
            reversibility: Reversibility::ReversibleVia {
                inverse: Box::new(ChangeKind::FeatureState {
                    name,
                    enable: !enable,
                    source: None,
                }),
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
    let mut b = PlanBuilder::new(
        target,
        os_build,
        descriptor("packages.remove", "op-packages-remove"),
    );
    let reason = b.atom("rev-package-remove")?;
    let mut planned: Vec<&str> = Vec::new();
    for wanted in identities {
        let package = current
            .iter()
            .find(|p| p.identity.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| PlanError::Unknown {
                what: "package",
                name: wanted.clone(),
            })?;
        if planned.contains(&package.identity.as_str()) {
            continue;
        }
        planned.push(&package.identity);
        let identity = b.atom(&package.identity)?;
        b.push(Change {
            kind: ChangeKind::PackageRemove { identity },
            reversibility: Reversibility::Irreversible { reason },
            reboot_required: true,
        })?;
    }
    Ok(b.finish())
}

/// Probes an interrupted job (no commit record): of the `in_doubt` intents, the feature
/// changes whose feature is now in the requested state, i.e. that were applied before the
/// crash. Other kinds can't be probed yet; they stay in doubt and undo skips them.
#[must_use]
pub fn probe_features(rec: &Recovered, in_doubt: &[u32], current: &[FeatureEntry]) -> Vec<u32> {
    rec.records
        .iter()
        .filter_map(|r| match r {
            Record::Intent(Intent {
                index,
                change:
                    Change {
                        kind: ChangeKind::FeatureState { name, enable, .. },
                        ..
                    },
                ..
            }) if in_doubt.contains(index) => current
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(name) && f.enabled == *enable)
                .then_some(*index),
            _ => None,
        })
        .collect()
}

fn descriptor(id: &str, title_key: &str) -> OpDescriptor {
    OpDescriptor {
        id: id.to_owned(),
        title_key: title_key.to_owned(),
        category: "dism".to_owned(),
        cancellable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: OsBuild = OsBuild {
        major: 10,
        minor: 0,
        build: 26100,
        revision: 0,
    };

    fn features() -> Vec<FeatureEntry> {
        vec![
            FeatureEntry {
                name: "SMB1Protocol".into(),
                enabled: true,
            },
            FeatureEntry {
                name: "TelnetClient".into(),
                enabled: false,
            },
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
            ChangeKind::FeatureState {
                name: "SMB1Protocol".into(),
                enable: false,
                source: None
            }
        );
        assert!(matches!(
            doc.changes[0].reversibility,
            Reversibility::ReversibleVia { .. }
        ));
    }

    #[test]
    fn unknown_names_fail_the_whole_plan() {
        let err = plan_features(
            TargetId([0; 16]),
            BUILD,
            &features(),
            &["Nope".to_owned()],
            true,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            PlanError::Unknown {
                what: "feature",
                ..
            }
        ));
    }

    #[test]
    fn repeated_names_are_planned_once() {
        let names = vec!["TelnetClient".to_owned(), "telnetclient".to_owned()];
        let plan = plan_features(TargetId([0; 16]), BUILD, &features(), &names, true).unwrap();
        assert_eq!(plan.summary().changes, 1);
    }

    #[test]
    fn probing_finds_interrupted_feature_changes_that_landed() {
        use dpro_core::{id::JobId, units::UnixMillis};

        use crate::journal::{Begin, JournalWriter, read};

        let plan = plan_features(
            TargetId([0; 16]),
            BUILD,
            &features(),
            &["TelnetClient".to_owned()],
            true,
        )
        .unwrap();
        let begin = Begin {
            job: JobId::from_parts(UnixMillis(1), 1),
            target: TargetId([0; 16]),
            os_build: BUILD,
            created: UnixMillis(1),
            op: "features.enable".into(),
            undoes: None,
        };
        let mut w = JournalWriter::create(Vec::new(), &begin).unwrap();
        let change = plan.to_doc().unwrap().changes.remove(0);
        w.intent(&Intent {
            index: 0,
            change,
            inverse: None,
        })
        .unwrap();
        w.flush().unwrap();
        // Crash: no done record, no commit.
        let rec = read(&w.commit(crate::journal::JobStatus::Aborted).unwrap()).unwrap();

        assert_eq!(probe_features(&rec, &[0], &features()), Vec::<u32>::new());
        let mut now = features();
        now[1].enabled = true;
        assert_eq!(probe_features(&rec, &[0], &now), [0]);
        assert_eq!(probe_features(&rec, &[], &now), Vec::<u32>::new());
    }

    #[test]
    fn package_removal_is_irreversible() {
        let current = vec![PackageEntry {
            identity: "Pkg~31bf3856ad364e35~amd64~~1.0".into(),
        }];
        let plan = plan_package_removal(
            TargetId([0; 16]),
            BUILD,
            &current,
            &["pkg~31bf3856ad364e35~amd64~~1.0".into()],
        )
        .unwrap();
        assert_eq!(plan.summary().irreversible, 1);
    }
}
