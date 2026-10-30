//! Servicing targets (doc 02 §4).

use std::path::PathBuf;

/// What DISM Pro services. Paths are as seen by the running OS.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    /// The running OS.
    Online { windows_dir: PathBuf },
    /// Another installation on an attached volume.
    OfflineInstall {
        /// Volume root, e.g. `D:\`.
        root: PathBuf,
        /// Relative to `root`, normally `Windows`.
        windows_dir: PathBuf,
    },
    /// An image mounted from a WIM/ESD/VHD.
    MountedImage {
        image: PathBuf,
        index: u32,
        mount_dir: PathBuf,
    },
}

impl Target {
    /// Stable kind name for logs, JSON and the target pill.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Online { .. } => "online",
            Self::OfflineInstall { .. } => "offline",
            Self::MountedImage { .. } => "image",
        }
    }
}
