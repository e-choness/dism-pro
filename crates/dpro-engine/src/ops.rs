//! Operations that plan DISM changes from the target's current state (TD-0001 §5.1, TD-0002 §4).
//! Pure: the caller lists features/packages/drivers through the target actor and passes them
//! in. [`collect_infs`] is the exception: it reads the folders `drivers add` names.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use dpro_core::{
    Error,
    id::TargetId,
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, Plan, PlanBuilder, Reversibility},
    units::UnixMillis,
};

use crate::{
    journal::{Intent, Learned, Record, Recovered},
    paths,
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

/// Whether Windows trusts a driver package's signature (`DismDriverSignature`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverSignature {
    Unknown,
    Unsigned,
    Signed,
}

impl DriverSignature {
    /// As in `drivers.v1`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Unsigned => "unsigned",
            Self::Signed => "signed",
        }
    }
}

/// A driver package as the planners see it: a staged package from `DismGetDrivers`, or an INF
/// read by `DismGetDriverInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverEntry {
    /// `oem3.inf` for a staged third-party package; an inbox package keeps its INF name.
    pub published: String,
    /// The INF's path: the store copy for a staged package, the given file for a loose INF.
    pub inf: String,
    pub inbox: bool,
    pub boot_critical: bool,
    pub signature: DriverSignature,
    pub provider: String,
    pub class: String,
    /// `DriverVer` version: major, minor, build, revision.
    pub version: [u32; 4],
    /// `DriverVer` date, `YYYY-MM-DD`.
    pub date: String,
}

impl DriverEntry {
    /// The INF's file name, whatever folder it's in. Windows paths: both separators count.
    #[must_use]
    pub fn inf_name(&self) -> &str {
        self.inf.rsplit(['\\', '/']).next().unwrap_or(&self.inf)
    }

    /// The same package as `other` by what the store records about it: INF file name,
    /// provider, class, version and date (TD-0002 §5.6).
    #[must_use]
    pub fn same_identity(&self, other: &Self) -> bool {
        self.inf_name().eq_ignore_ascii_case(other.inf_name())
            && self.provider == other.provider
            && self.class.eq_ignore_ascii_case(&other.class)
            && self.version == other.version
            && self.date == other.date
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error(transparent)]
    Core(#[from] Error),
    #[error("unknown {what}: {name}")]
    Unknown { what: &'static str, name: String },
    #[error("{inf} isn't signed (signature: {signature}). Pass --force-unsigned to add it anyway")]
    Unsigned {
        inf: String,
        signature: &'static str,
    },
    #[error("{name} ships with Windows and can't be removed")]
    Inbox { name: String },
    #[error(
        "{name} is boot-critical: without it the image may not start. Pass \
         --allow-boot-critical to remove it anyway"
    )]
    BootCritical { name: String },
    #[error("{path} isn't an .inf file")]
    NotInf { path: String },
    #[error("{folder} has no .inf files")]
    NoInf { folder: String },
    #[error("reading {path}: {source}")]
    Io { path: String, source: io::Error },
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

/// The INFs `drivers add` names, sorted and de-duplicated so a plan doesn't depend on directory
/// order. A file must be an `.inf`; a folder gives its `*.inf` files, and its subfolders' with
/// `recurse`. Links inside a folder are not followed. A folder without INFs is an error, not an
/// empty plan.
///
/// # Errors
/// [`PlanError::NotInf`], [`PlanError::NoInf`], or [`PlanError::Io`] for a path that can't be
/// read.
pub fn collect_infs(paths: &[PathBuf], recurse: bool) -> Result<Vec<PathBuf>, PlanError> {
    fn is_inf(p: &Path) -> bool {
        p.extension().is_some_and(|e| e.eq_ignore_ascii_case("inf"))
    }
    fn walk(dir: &Path, recurse: bool, out: &mut Vec<PathBuf>) -> Result<(), PlanError> {
        let io = |source| PlanError::Io {
            path: dir.display().to_string(),
            source,
        };
        for entry in fs::read_dir(dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            // `file_type` doesn't follow links: a junction or symlink is neither file nor dir.
            let kind = entry.file_type().map_err(io)?;
            let path = entry.path();
            if kind.is_file() && is_inf(&path) {
                out.push(path);
            } else if kind.is_dir() && recurse {
                walk(&path, recurse, out)?;
            }
        }
        Ok(())
    }

    let mut out = Vec::new();
    for path in paths {
        let md = fs::metadata(path).map_err(|source| PlanError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if md.is_dir() {
            let before = out.len();
            walk(path, recurse, &mut out)?;
            if out.len() == before {
                return Err(PlanError::NoInf {
                    folder: path.display().to_string(),
                });
            }
        } else if is_inf(path) {
            out.push(path.clone());
        } else {
            return Err(PlanError::NotInf {
                path: path.display().to_string(),
            });
        }
    }
    let key = |p: &PathBuf| p.to_string_lossy().to_lowercase();
    out.sort_by_cached_key(key);
    out.dedup_by(|a, b| key(a) == key(b));
    Ok(out)
}

/// `drivers add` (TD-0002 §5.6): one `DriverAdd` per INF in `candidates`, each read by
/// `DismGetDriverInfo`. An unsigned or unknown-signature package is refused unless
/// `force_unsigned`: the plan is the consent, and DISM's own refusal names the wrong cause. A
/// package already in the store is skipped: same identity ([`DriverEntry::same_identity`]) and
/// `same_bytes(candidate INF, store INF)`. The inverse is learned when the add runs, because
/// DISM names the package (`oemN.inf`) then.
///
/// # Errors
/// [`PlanError::Unsigned`].
pub fn plan_driver_add(
    target: TargetId,
    os_build: OsBuild,
    staged: &[DriverEntry],
    candidates: &[DriverEntry],
    force_unsigned: bool,
    mut same_bytes: impl FnMut(&str, &str) -> bool,
) -> Result<Plan, PlanError> {
    let mut b = PlanBuilder::new(
        target,
        os_build,
        descriptor("drivers.add", "op-drivers-add"),
    );
    let mut planned: Vec<&str> = Vec::new();
    for c in candidates {
        if c.signature != DriverSignature::Signed && !force_unsigned {
            return Err(PlanError::Unsigned {
                inf: c.inf.clone(),
                signature: c.signature.name(),
            });
        }
        let duplicate = planned.iter().any(|p| p.eq_ignore_ascii_case(&c.inf));
        let in_store = staged
            .iter()
            .any(|s| s.same_identity(c) && same_bytes(&c.inf, &s.inf));
        if duplicate || in_store {
            continue;
        }
        planned.push(&c.inf);
        let inf = b.atom(&c.inf)?;
        b.push(Change {
            kind: ChangeKind::DriverAdd { inf },
            reversibility: Reversibility::Reversible,
            // Staging into an offline image's store needs no restart.
            reboot_required: false,
        })?;
    }
    Ok(b.finish())
}

/// `drivers remove <published>...` (TD-0002 §5.6). Names match case-insensitively, like
/// `DISM /Remove-Driver`. Inbox packages are refused (DISM can't remove them), and so are
/// boot-critical ones unless `allow_boot_critical`. Each removal copies the package's store
/// folder into quarantine first, so it's reversible until the quarantine TTL.
///
/// # Errors
/// [`PlanError::Unknown`], [`PlanError::Inbox`], [`PlanError::BootCritical`].
pub fn plan_driver_removal(
    target: TargetId,
    os_build: OsBuild,
    staged: &[DriverEntry],
    names: &[String],
    allow_boot_critical: bool,
    now: UnixMillis,
) -> Result<Plan, PlanError> {
    let mut b = PlanBuilder::new(
        target,
        os_build,
        descriptor("drivers.remove", "op-drivers-remove"),
    );
    let until = paths::quarantine_deadline(now);
    let mut planned: Vec<&str> = Vec::new();
    for wanted in names {
        let package = staged
            .iter()
            .find(|p| p.published.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| PlanError::Unknown {
                what: "third-party driver package",
                name: wanted.clone(),
            })?;
        if package.inbox {
            return Err(PlanError::Inbox {
                name: package.published.clone(),
            });
        }
        if package.boot_critical && !allow_boot_critical {
            return Err(PlanError::BootCritical {
                name: package.published.clone(),
            });
        }
        if planned.contains(&package.published.as_str()) {
            continue;
        }
        planned.push(&package.published);
        let published = b.atom(&package.published)?;
        b.push(Change {
            kind: ChangeKind::DriverRemove { published },
            reversibility: Reversibility::ReversibleUntil { until },
            reboot_required: false,
        })?;
    }
    Ok(b.finish())
}

/// What probing an interrupted job found: intents that were applied, and the inverses of
/// applied driver adds whose learned record didn't survive.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Probed {
    pub applied: Vec<u32>,
    pub learned: Vec<Learned>,
}

/// The driver changes among `in_doubt` that landed before the crash (TD-0002 §5.6). A removal
/// landed if its package is no longer staged. An add landed if exactly one staged package has
/// the source INF's identity; `identity_of` reads it (`DismGetDriverInfo`), and `None` (the
/// source is gone) leaves the add in doubt, so undo skips it.
pub fn probe_drivers(
    rec: &Recovered,
    in_doubt: &[u32],
    staged: &[DriverEntry],
    mut identity_of: impl FnMut(&str) -> Option<DriverEntry>,
) -> Probed {
    let mut probed = Probed::default();
    for r in &rec.records {
        let Record::Intent(Intent { index, change, .. }) = r else {
            continue;
        };
        if !in_doubt.contains(index) {
            continue;
        }
        match &change.kind {
            ChangeKind::DriverRemove { published } => {
                if !staged
                    .iter()
                    .any(|s| s.published.eq_ignore_ascii_case(published))
                {
                    probed.applied.push(*index);
                }
            }
            ChangeKind::DriverAdd { inf } => {
                let Some(source) = identity_of(inf) else {
                    continue;
                };
                let mut matches = staged.iter().filter(|s| s.same_identity(&source));
                if let (Some(found), None) = (matches.next(), matches.next()) {
                    probed.applied.push(*index);
                    probed.learned.push(Learned {
                        index: *index,
                        inverse: ChangeKind::DriverRemove {
                            published: found.published.clone(),
                        },
                    });
                }
            }
            _ => {}
        }
    }
    probed
}

/// Whether any of the `in_doubt` intents has a kind `pick` selects: probing lists the target
/// only for kinds that are there (a feature listing can take a minute).
#[must_use]
pub fn in_doubt_has(
    rec: &Recovered,
    in_doubt: &[u32],
    pick: impl Fn(&ChangeKind<String>) -> bool,
) -> bool {
    rec.records.iter().any(|r| {
        matches!(r, Record::Intent(Intent { index, change, .. })
            if in_doubt.contains(index) && pick(&change.kind))
    })
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

    fn driver(published: &str, inf: &str) -> DriverEntry {
        DriverEntry {
            published: published.into(),
            inf: inf.into(),
            inbox: false,
            boot_critical: false,
            signature: DriverSignature::Signed,
            provider: "Contoso".into(),
            class: "System".into(),
            version: [1, 0, 0, 0],
            date: "2026-11-03".into(),
        }
    }

    fn store_copy(name: &str) -> String {
        format!(r"C:\Windows\System32\DriverStore\FileRepository\{name}_amd64_0123\{name}")
    }

    #[test]
    fn identity_ignores_the_folder_and_case() {
        let staged = driver("oem3.inf", &store_copy("net.inf"));
        let loose = driver("", r"D:\drivers\NET.INF");
        assert_eq!(loose.inf_name(), "NET.INF");
        assert!(staged.same_identity(&loose));
        let newer = DriverEntry {
            version: [1, 0, 0, 1],
            ..loose.clone()
        };
        assert!(!staged.same_identity(&newer));
    }

    #[test]
    fn driver_add_gates_signatures_and_skips_what_is_staged() {
        let target = TargetId([0; 16]);
        let staged = [driver("oem0.inf", &store_copy("a.inf"))];
        let a = driver("", r"D:\d\a.inf");
        let b = DriverEntry {
            signature: DriverSignature::Unsigned,
            ..driver("", r"D:\d\b.inf")
        };
        let same = |_: &str, _: &str| true;

        let err = plan_driver_add(target, BUILD, &staged, &[a.clone(), b.clone()], false, same)
            .unwrap_err();
        assert!(
            matches!(err, PlanError::Unsigned { ref inf, signature: "unsigned" } if inf == r"D:\d\b.inf")
        );

        // a is staged byte for byte: only b is planned, and its inverse is learned later.
        let plan = plan_driver_add(
            target,
            BUILD,
            &staged,
            &[a.clone(), b.clone(), b.clone()],
            true,
            same,
        )
        .unwrap();
        let doc = plan.to_doc().unwrap();
        assert_eq!(doc.changes.len(), 1);
        assert_eq!(
            doc.changes[0].kind,
            ChangeKind::DriverAdd {
                inf: r"D:\d\b.inf".into()
            }
        );
        assert_eq!(doc.changes[0].reversibility, Reversibility::Reversible);
        assert!(!plan.summary().reboot_required);

        // Same identity, different bytes: a real change, so it's planned.
        let mut compared = Vec::new();
        let plan = plan_driver_add(target, BUILD, &staged, &[a], false, |x: &str, y: &str| {
            compared.push((x.to_owned(), y.to_owned()));
            false
        })
        .unwrap();
        assert_eq!(plan.summary().changes, 1);
        assert_eq!(compared, [(r"D:\d\a.inf".to_owned(), store_copy("a.inf"))]);
    }

    #[test]
    fn driver_removal_refuses_inbox_boot_critical_and_unknown() {
        let target = TargetId([0; 16]);
        let staged = [
            driver("oem1.inf", &store_copy("a.inf")),
            DriverEntry {
                boot_critical: true,
                ..driver("oem2.inf", &store_copy("disk.inf"))
            },
            DriverEntry {
                inbox: true,
                ..driver("machine.inf", &store_copy("machine.inf"))
            },
        ];
        let remove = |names: &[&str], allow: bool| {
            let names: Vec<String> = names.iter().map(|&n| n.to_owned()).collect();
            plan_driver_removal(target, BUILD, &staged, &names, allow, UnixMillis(1000))
        };
        assert!(matches!(
            remove(&["oem9.inf"], true),
            Err(PlanError::Unknown { .. })
        ));
        assert!(matches!(
            remove(&["machine.inf"], true),
            Err(PlanError::Inbox { .. })
        ));
        assert!(matches!(
            remove(&["OEM2.INF"], false),
            Err(PlanError::BootCritical { .. })
        ));

        let plan = remove(&["OEM1.inf", "oem2.inf", "oem1.inf"], true).unwrap();
        let doc = plan.to_doc().unwrap();
        let names: Vec<_> = doc.changes.iter().map(|c| c.kind.clone()).collect();
        assert_eq!(
            names,
            [
                ChangeKind::DriverRemove {
                    published: "oem1.inf".into()
                },
                ChangeKind::DriverRemove {
                    published: "oem2.inf".into()
                }
            ]
        );
        assert_eq!(
            plan.summary().earliest_deadline,
            Some(paths::quarantine_deadline(UnixMillis(1000)))
        );
    }

    #[test]
    fn infs_are_collected_sorted_and_once() {
        let root = std::env::temp_dir().join(format!("dpro-infs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root); // ignore: may not exist
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        for f in ["b.inf", "A.INF", "readme.txt"] {
            fs::write(root.join(f), b"x").unwrap();
        }
        fs::write(sub.join("c.inf"), b"x").unwrap();

        let names = |v: Vec<PathBuf>| -> Vec<String> {
            v.iter()
                .map(|p| {
                    p.strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        };
        let flat = collect_infs(&[root.clone(), root.join("b.inf")], false).unwrap();
        assert_eq!(names(flat), ["A.INF", "b.inf"]);
        let deep = collect_infs(std::slice::from_ref(&root), true).unwrap();
        assert_eq!(names(deep), ["A.INF", "b.inf", "sub/c.inf"]);
        assert!(matches!(
            collect_infs(&[root.join("empty")], true),
            Err(PlanError::NoInf { .. })
        ));
        assert!(matches!(
            collect_infs(&[root.join("readme.txt")], false),
            Err(PlanError::NotInf { .. })
        ));
        assert!(matches!(
            collect_infs(&[root.join("missing.inf")], false),
            Err(PlanError::Io { .. })
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn probing_finds_driver_changes_that_landed() {
        use dpro_core::{id::JobId, units::UnixMillis};

        use crate::journal::{Begin, JobStatus, JournalWriter, read};

        let begin = Begin {
            job: JobId::from_parts(UnixMillis(1), 1),
            target: TargetId([0; 16]),
            os_build: BUILD,
            created: UnixMillis(1),
            op: "drivers.add".into(),
            undoes: None,
        };
        let intent = |index, kind| Intent {
            index,
            change: Change {
                kind,
                reversibility: Reversibility::Reversible,
                reboot_required: false,
            },
            inverse: None,
        };
        let mut w = JournalWriter::create(Vec::new(), &begin).unwrap();
        w.intent(&intent(
            0,
            ChangeKind::DriverAdd {
                inf: r"D:\d\a.inf".into(),
            },
        ))
        .unwrap();
        w.intent(&intent(
            1,
            ChangeKind::DriverAdd {
                inf: r"D:\d\gone.inf".into(),
            },
        ))
        .unwrap();
        w.intent(&intent(
            2,
            ChangeKind::DriverRemove {
                published: "oem7.inf".into(),
            },
        ))
        .unwrap();
        w.intent(&intent(
            3,
            ChangeKind::DriverRemove {
                published: "oem8.inf".into(),
            },
        ))
        .unwrap();
        w.flush().unwrap();
        let rec = read(&w.commit(JobStatus::Aborted).unwrap()).unwrap();

        // a.inf is staged as oem4.inf; oem7.inf is gone; oem8.inf is still there.
        let staged = [
            driver("oem4.inf", &store_copy("a.inf")),
            driver("oem8.inf", &store_copy("x.inf")),
        ];
        let identity = |inf: &str| (inf == r"D:\d\a.inf").then(|| driver("", inf));
        let probed = probe_drivers(&rec, &[0, 1, 2, 3], &staged, identity);
        assert_eq!(probed.applied, [0, 2]);
        assert_eq!(
            probed.learned,
            [Learned {
                index: 0,
                inverse: ChangeKind::DriverRemove {
                    published: "oem4.inf".into()
                }
            }]
        );
        assert!(in_doubt_has(&rec, &[3], |k| matches!(
            k,
            ChangeKind::DriverRemove { .. }
        )));
        assert!(!in_doubt_has(&rec, &[0, 1, 2, 3], |k| matches!(
            k,
            ChangeKind::FeatureState { .. }
        )));
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
