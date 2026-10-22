//! Target identity: `TargetId` from `MachineGuid` and `OsBuild` from `CurrentVersion`
//! (TD-0001 §5.1). Pure over [`RegistryView`].

use dpro_core::{id::TargetId, plan::OsBuild};

use crate::registry::{Hive, RegistryView, Value};

const CURRENT_VERSION: &str = r"Microsoft\Windows NT\CurrentVersion";

/// `SOFTWARE\Microsoft\Cryptography\MachineGuid`, or `None` if absent/garbled (then the
/// caller falls back to a path-derived id and marks the target "unstable id").
#[must_use]
pub fn target_id(reg: &impl RegistryView) -> Option<TargetId> {
    match reg.value(Hive::Software, r"Microsoft\Cryptography", "MachineGuid")? {
        Value::Str(s) => s.trim_matches(['{', '}']).parse().ok(),
        _ => None,
    }
}

/// Major/minor from `CurrentMajorVersionNumber`/`CurrentMinorVersionNumber` (Windows 10+),
/// build from `CurrentBuildNumber` (a string), revision from `UBR`.
#[must_use]
pub fn os_build(reg: &impl RegistryView) -> Option<OsBuild> {
    let dword = |name: &str| match reg.value(Hive::Software, CURRENT_VERSION, name) {
        Some(Value::Dword(v)) => Some(v),
        _ => None,
    };
    let build = match reg.value(Hive::Software, CURRENT_VERSION, "CurrentBuildNumber")? {
        Value::Str(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    Some(OsBuild {
        major: dword("CurrentMajorVersionNumber").unwrap_or(10),
        minor: dword("CurrentMinorVersionNumber").unwrap_or(0),
        build,
        revision: dword("UBR").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FakeRegistry;

    #[test]
    fn reads_identity() {
        let reg = FakeRegistry::default()
            .with_value(Hive::Software, r"Microsoft\Cryptography", "MachineGuid", Value::Str("53bfae52-b167-4e2f-a258-0a37b57ff845".into()))
            .with_value(Hive::Software, CURRENT_VERSION, "CurrentMajorVersionNumber", Value::Dword(10))
            .with_value(Hive::Software, CURRENT_VERSION, "CurrentBuildNumber", Value::Str("26100".into()))
            .with_value(Hive::Software, CURRENT_VERSION, "UBR", Value::Dword(2033));
        assert_eq!(target_id(&reg).unwrap().to_string(), "53bfae52-b167-4e2f-a258-0a37b57ff845");
        assert_eq!(os_build(&reg), Some(OsBuild { major: 10, minor: 0, build: 26100, revision: 2033 }));
    }

    #[test]
    fn missing_values_are_none() {
        let reg = FakeRegistry::default();
        assert_eq!(target_id(&reg), None);
        assert_eq!(os_build(&reg), None);
    }
}
