//! Why a reboot is pending (TD-0006 §3, F-16). Pure over [`RegistryView`] + [`FileView`], so
//! it works for the running OS and for offline installs (their control set via `Select`).

use std::path::Path;

use dpro_core::health::RebootReason;

use crate::registry::{Hive, RegistryView, Value};

pub trait FileView {
    fn exists(&self, path: &Path) -> bool;
}

/// `std::fs` (for the running OS or a mounted offline volume).
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFiles;

impl FileView for RealFiles {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

const CBS: &str = r"Microsoft\Windows\CurrentVersion\Component Based Servicing";
const WU: &str = r"Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired";

/// Every pending-reboot signal found, in a stable order.
#[must_use]
pub fn reboot_reasons(reg: &impl RegistryView, files: &impl FileView, windows_dir: &Path) -> Vec<RebootReason> {
    let mut out = Vec::new();
    if reg.key_exists(Hive::Software, &format!(r"{CBS}\RebootPending")) {
        out.push(RebootReason::Cbs);
    }
    if reg.key_exists(Hive::Software, &format!(r"{CBS}\PackagesPending")) {
        out.push(RebootReason::CbsPackagesPending);
    }
    if files.exists(&windows_dir.join("WinSxS").join("pending.xml")) {
        out.push(RebootReason::PendingXml);
    }
    if reg.key_exists(Hive::Software, WU) {
        out.push(RebootReason::WindowsUpdate);
    }
    let cs = reg.control_set();
    let session_manager = format!(r"{cs}\Control\Session Manager");
    let renames: usize = ["PendingFileRenameOperations", "PendingFileRenameOperations2"]
        .iter()
        .map(|name| match reg.value(Hive::System, &session_manager, name) {
            // Pairs of (source, destination); an empty destination means delete.
            Some(Value::MultiStr(entries)) => entries.len().div_ceil(2),
            _ => 0,
        })
        .sum();
    if renames > 0 {
        out.push(RebootReason::FileRename { count: u32::try_from(renames).unwrap_or(u32::MAX) });
    }
    let name_key = |sub: &str| reg.value(Hive::System, &format!(r"{cs}\Control\ComputerName\{sub}"), "ComputerName");
    // The active name is volatile: absent offline, so no comparison there.
    if let (Some(Value::Str(active)), Some(Value::Str(pending))) = (name_key("ActiveComputerName"), name_key("ComputerName"))
        && !active.eq_ignore_ascii_case(&pending)
    {
        out.push(RebootReason::ComputerRename);
    }
    if matches!(reg.value(Hive::Software, r"Microsoft\Updates", "UpdateExeVolatile"), Some(Value::Dword(v)) if v != 0) {
        out.push(RebootReason::UpdateExeVolatile);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use super::*;
    use crate::registry::FakeRegistry;

    struct Files(HashSet<PathBuf>);

    impl FileView for Files {
        fn exists(&self, path: &Path) -> bool {
            self.0.contains(path)
        }
    }

    #[test]
    fn clean_system_has_no_reasons() {
        let reg = FakeRegistry::default().with_key(Hive::System, "CurrentControlSet");
        assert!(reboot_reasons(&reg, &Files(HashSet::new()), Path::new(r"C:\Windows")).is_empty());
    }

    #[test]
    fn every_signal_offline() {
        let win = PathBuf::from(r"D:\Windows");
        let cs = "ControlSet002";
        let reg = FakeRegistry::default()
            .with_value(Hive::System, "Select", "Current", Value::Dword(2))
            .with_key(Hive::Software, &format!(r"{CBS}\RebootPending"))
            .with_key(Hive::Software, WU)
            .with_value(
                Hive::System,
                &format!(r"{cs}\Control\Session Manager"),
                "PendingFileRenameOperations",
                Value::MultiStr(vec![r"\??\C:\a".into(), String::new(), r"\??\C:\b".into(), r"\??\C:\c".into()]),
            )
            .with_value(Hive::Software, r"Microsoft\Updates", "UpdateExeVolatile", Value::Dword(1));
        let files = Files([win.join("WinSxS").join("pending.xml")].into_iter().collect());
        let reasons = reboot_reasons(&reg, &files, &win);
        assert_eq!(
            reasons,
            [
                RebootReason::Cbs,
                RebootReason::PendingXml,
                RebootReason::WindowsUpdate,
                RebootReason::FileRename { count: 2 },
                RebootReason::UpdateExeVolatile,
            ]
        );
    }

    #[test]
    fn computer_rename_online() {
        let cs = "CurrentControlSet";
        let reg = FakeRegistry::default()
            .with_key(Hive::System, cs)
            .with_value(Hive::System, &format!(r"{cs}\Control\ComputerName\ActiveComputerName"), "ComputerName", Value::Str("OLD".into()))
            .with_value(Hive::System, &format!(r"{cs}\Control\ComputerName\ComputerName"), "ComputerName", Value::Str("NEW".into()));
        assert_eq!(reboot_reasons(&reg, &Files(HashSet::new()), Path::new("C:")), [RebootReason::ComputerRename]);
    }
}
