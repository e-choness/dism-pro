//! Target discovery (roadmap 2.4): the running OS plus offline installations on other
//! volumes. Mounted images are listed by the DISM API and added by the caller.

use std::path::{Path, PathBuf};

use dpro_core::target::Target;

/// What discovery needs from the system; faked in tests.
pub trait SystemProbe {
    /// The running OS's Windows directory.
    fn windows_directory(&self) -> Option<PathBuf>;
    /// `C:\`, `D:\`, … for assigned drive letters.
    fn drive_roots(&self) -> Vec<PathBuf>;
    fn is_file(&self, path: &Path) -> bool;
}

/// Relative path whose presence marks an offline installation.
const SYSTEM_HIVE: [&str; 4] = ["Windows", "System32", "config", "SYSTEM"];

/// Online target first, then offline installs in drive order. A volume counts as an
/// offline install if it has `Windows\System32\config\SYSTEM` and isn't the online OS's.
#[must_use]
pub fn discover(probe: &impl SystemProbe) -> Vec<Target> {
    let online = probe.windows_directory();
    let online_root = online.as_deref().and_then(Path::parent).map(normalize);
    let mut out = Vec::new();
    if let Some(windows_dir) = online {
        out.push(Target::Online { windows_dir });
    }
    for root in probe.drive_roots() {
        if online_root.as_deref() == Some(normalize(&root).as_str()) {
            continue;
        }
        let hive = SYSTEM_HIVE.iter().fold(root.clone(), |p, c| p.join(c));
        if probe.is_file(&hive) {
            out.push(Target::OfflineInstall { root, windows_dir: PathBuf::from(SYSTEM_HIVE[0]) });
        }
    }
    out
}

/// Case-insensitive comparison key for a volume root (`c:\`, `C:` → `c:`).
fn normalize(p: &Path) -> String {
    p.to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase()
}

/// The real system, through `dpro-win`.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsSystem;

#[cfg(windows)]
impl SystemProbe for WindowsSystem {
    fn windows_directory(&self) -> Option<PathBuf> {
        dpro_win::system::windows_directory().ok()
    }

    fn drive_roots(&self) -> Vec<PathBuf> {
        dpro_win::system::drive_roots()
    }

    fn is_file(&self, path: &Path) -> bool {
        dpro_win::system::probe(path) == Some(dpro_win::system::PathKind::File)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    struct Fake {
        windows: Option<PathBuf>,
        roots: Vec<PathBuf>,
        files: HashSet<PathBuf>,
    }

    impl SystemProbe for Fake {
        fn windows_directory(&self) -> Option<PathBuf> {
            self.windows.clone()
        }
        fn drive_roots(&self) -> Vec<PathBuf> {
            self.roots.clone()
        }
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
    }

    fn hive(root: &str) -> PathBuf {
        SYSTEM_HIVE.iter().fold(PathBuf::from(root), |p, c| p.join(c))
    }

    #[test]
    fn finds_online_and_offline_installs() {
        let fake = Fake {
            windows: Some(PathBuf::from(r"C:\").join("Windows")),
            roots: [r"C:\", r"D:\", r"E:\"].map(PathBuf::from).to_vec(),
            // C: is the online OS (its hive must not make it "offline"); E: has no Windows.
            files: [hive(r"C:\"), hive(r"D:\")].into_iter().collect(),
        };
        let targets = discover(&fake);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].kind(), "online");
        assert_eq!(
            targets[1],
            Target::OfflineInstall { root: PathBuf::from(r"D:\"), windows_dir: PathBuf::from("Windows") }
        );
    }

    #[test]
    fn works_without_an_online_os() {
        // e.g. WinPE where the Windows directory couldn't be read.
        let fake = Fake { windows: None, roots: vec![PathBuf::from(r"D:\")], files: [hive(r"D:\")].into_iter().collect() };
        let targets = discover(&fake);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].kind(), "offline");
    }
}
