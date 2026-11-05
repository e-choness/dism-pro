//! Read-only registry access as a trait, so identity and reboot-reason logic is pure and
//! tested with fakes. `dpro-win` backs it for the running OS, and the `regf` reader for an
//! offline installation's hive files.

use std::{collections::HashMap, path::Path};

use crate::regf::{Regf, RegfError};

/// The two machine hives the engine reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    /// `HKLM\SOFTWARE` online, `config\SOFTWARE` offline.
    Software,
    /// `HKLM\SYSTEM` online, `config\SYSTEM` offline.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    MultiStr(Vec<String>),
    Dword(u32),
    Qword(u64),
    Binary(Vec<u8>),
    Other,
}

pub trait RegistryView {
    /// `key` is relative to the hive root, `\`-separated.
    fn key_exists(&self, hive: Hive, key: &str) -> bool;
    fn value(&self, hive: Hive, key: &str, name: &str) -> Option<Value>;

    /// `CurrentControlSet` online; offline hives only have `ControlSet00N`, chosen by
    /// `Select\Current` (TD-0006 §3).
    fn control_set(&self) -> String {
        if self.key_exists(Hive::System, "CurrentControlSet") {
            return "CurrentControlSet".to_owned();
        }
        match self.value(Hive::System, "Select", "Current") {
            Some(Value::Dword(n)) => format!("ControlSet{n:03}"),
            _ => "ControlSet001".to_owned(),
        }
    }
}

/// In-memory registry for tests and fixtures: keys are `(hive, lower-case key)`.
#[derive(Debug, Clone, Default)]
pub struct FakeRegistry {
    values: HashMap<(Hive, String), HashMap<String, Value>>,
}

impl FakeRegistry {
    #[must_use]
    pub fn with_key(mut self, hive: Hive, key: &str) -> Self {
        self.values
            .entry((hive, key.to_ascii_lowercase()))
            .or_default();
        self
    }

    #[must_use]
    pub fn with_value(mut self, hive: Hive, key: &str, name: &str, value: Value) -> Self {
        self.values
            .entry((hive, key.to_ascii_lowercase()))
            .or_default()
            .insert(name.to_ascii_lowercase(), value);
        self
    }
}

impl RegistryView for FakeRegistry {
    fn key_exists(&self, hive: Hive, key: &str) -> bool {
        self.values.contains_key(&(hive, key.to_ascii_lowercase()))
    }

    fn value(&self, hive: Hive, key: &str, name: &str) -> Option<Value> {
        self.values
            .get(&(hive, key.to_ascii_lowercase()))?
            .get(&name.to_ascii_lowercase())
            .cloned()
    }
}

const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
const REG_DWORD_BIG_ENDIAN: u32 = 5;
const REG_MULTI_SZ: u32 = 7;
const REG_QWORD: u32 = 11;

/// Splits at NUL, as the registry stores strings: one for `REG_SZ`, a list for `REG_MULTI_SZ`.
fn strings(data: &[u8]) -> Vec<String> {
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    units
        .split(|unit| *unit == 0)
        .map(String::from_utf16_lossy)
        .collect()
}

/// A value as the file stores it. Data of the wrong size for its type reads as [`Value::Other`].
fn decode(kind: u32, data: &[u8]) -> Value {
    let dword = |data: &[u8]| <[u8; 4]>::try_from(data).ok();
    match kind {
        REG_SZ | REG_EXPAND_SZ => Value::Str(strings(data).into_iter().next().unwrap_or_default()),
        REG_MULTI_SZ => {
            let mut items = strings(data);
            // A well-formed value ends with "\0\0": drop the empty tail entries.
            while items.last().is_some_and(String::is_empty) {
                items.pop();
            }
            Value::MultiStr(items)
        }
        REG_DWORD => dword(data).map_or(Value::Other, |b| Value::Dword(u32::from_le_bytes(b))),
        REG_DWORD_BIG_ENDIAN => {
            dword(data).map_or(Value::Other, |b| Value::Dword(u32::from_be_bytes(b)))
        }
        REG_QWORD => data
            .try_into()
            .map_or(Value::Other, |b| Value::Qword(u64::from_le_bytes(b))),
        REG_BINARY => Value::Binary(data.to_vec()),
        _ => Value::Other,
    }
}

/// Values of one key in an offline installation's `SOFTWARE` hive, read from the file without
/// loading `SYSTEM` (TD-0013 §7.1: the servicing stamp before every cached answer). One entry
/// per name, `None` for a value that's missing (all of them when the key is).
///
/// # Errors
/// The hive file is missing, unreadable or not a hive, or the key's cells are damaged.
pub fn offline_software_values(
    windows_dir: &Path,
    key: &str,
    names: &[&str],
) -> Result<Vec<Option<Value>>, RegfError> {
    let path = windows_dir.join("System32").join("config").join("SOFTWARE");
    let hive = Regf::open(&path)?;
    let Some(key) = hive.key(key)? else {
        return Ok(vec![None; names.len()]);
    };
    names
        .iter()
        .map(|name| {
            key.value(name)?
                .map(|v| Ok(decode(v.kind()?, &v.data()?)))
                .transpose()
        })
        .collect()
}

/// An offline installation's `SOFTWARE` and `SYSTEM`, read from their files (ADR-0008). Both
/// stay in memory, so keep it for as long as the reads take.
#[derive(Debug)]
pub struct OfflineRegistry {
    software: Regf,
    system: Regf,
}

impl OfflineRegistry {
    /// `windows_dir` is the absolute Windows folder of the offline install.
    ///
    /// # Errors
    /// A hive file is missing, unreadable or not a hive.
    pub fn load(windows_dir: &Path) -> Result<Self, RegfError> {
        let config = windows_dir.join("System32").join("config");
        Ok(Self {
            software: Regf::open(&config.join("SOFTWARE"))?,
            system: Regf::open(&config.join("SYSTEM"))?,
        })
    }

    /// The installation wasn't shut down cleanly: its last registry changes are in the hive
    /// logs, and reads see the state before them.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.software.is_dirty() || self.system.is_dirty()
    }

    fn hive(&self, hive: Hive) -> &Regf {
        match hive {
            Hive::Software => &self.software,
            Hive::System => &self.system,
        }
    }
}

impl RegistryView for OfflineRegistry {
    fn key_exists(&self, hive: Hive, key: &str) -> bool {
        matches!(self.hive(hive).key(key), Ok(Some(_)))
    }

    fn value(&self, hive: Hive, key: &str, name: &str) -> Option<Value> {
        let key = self.hive(hive).key(key).ok()??;
        let value = key.value(name).ok()??;
        Some(decode(value.kind().ok()?, &value.data().ok()?))
    }
}

#[cfg(windows)]
pub use win::OnlineRegistry;

#[cfg(windows)]
mod win {
    use dpro_win::reg::{Access, RegKey, RegValue};

    use super::{Hive, RegistryView, Value};

    fn convert(v: RegValue) -> Value {
        match v {
            RegValue::Sz(s) | RegValue::ExpandSz(s) => Value::Str(s.to_string_lossy().into_owned()),
            RegValue::MultiSz(v) => {
                Value::MultiStr(v.iter().map(|s| s.to_string_lossy().into_owned()).collect())
            }
            RegValue::Dword(d) => Value::Dword(d),
            RegValue::Qword(q) => Value::Qword(q),
            RegValue::Binary(b) => Value::Binary(b),
            RegValue::Other { .. } => Value::Other,
        }
    }

    fn read(root: &RegKey, key: &str, name: &str) -> Option<Value> {
        let k = if key.is_empty() {
            None
        } else {
            Some(root.open(key, Access::Read).ok()?)
        };
        k.as_ref()
            .unwrap_or(root)
            .get(name)
            .ok()
            .flatten()
            .map(convert)
    }

    /// The running OS's `HKLM\SOFTWARE` and `HKLM\SYSTEM`.
    #[derive(Debug)]
    pub struct OnlineRegistry {
        software: RegKey,
        system: RegKey,
    }

    impl OnlineRegistry {
        /// # Errors
        /// The hive roots can't be opened.
        pub fn open() -> dpro_win::error::Result<Self> {
            let hklm = RegKey::local_machine();
            Ok(Self {
                software: hklm.open("SOFTWARE", Access::Read)?,
                system: hklm.open("SYSTEM", Access::Read)?,
            })
        }
    }

    impl RegistryView for OnlineRegistry {
        fn key_exists(&self, hive: Hive, key: &str) -> bool {
            let root = match hive {
                Hive::Software => &self.software,
                Hive::System => &self.system,
            };
            root.open(key, Access::Read).is_ok()
        }

        fn value(&self, hive: Hive, key: &str, name: &str) -> Option<Value> {
            let root = match hive {
                Hive::Software => &self.software,
                Hive::System => &self.system,
            };
            read(root, key, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        identity,
        regf::testkit::{TestKey, build},
    };

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn decodes_each_type() {
        assert_eq!(decode(REG_SZ, &utf16("a\0")), Value::Str("a".into()));
        assert_eq!(
            decode(REG_SZ, &utf16("no nul")),
            Value::Str("no nul".into())
        );
        assert_eq!(decode(REG_SZ, &[]), Value::Str(String::new()));
        assert_eq!(
            decode(REG_EXPAND_SZ, &utf16("%x%\0")),
            Value::Str("%x%".into())
        );
        assert_eq!(
            decode(REG_MULTI_SZ, &utf16("a\0b\0\0")),
            Value::MultiStr(vec!["a".into(), "b".into()])
        );
        assert_eq!(decode(REG_MULTI_SZ, &[]), Value::MultiStr(Vec::new()));
        assert_eq!(decode(REG_DWORD, &[1, 2, 0, 0]), Value::Dword(0x201));
        assert_eq!(
            decode(REG_DWORD_BIG_ENDIAN, &[0, 0, 2, 1]),
            Value::Dword(0x201)
        );
        assert_eq!(decode(REG_DWORD, &[1, 2]), Value::Other);
        assert_eq!(
            decode(REG_QWORD, &[1, 0, 0, 0, 0, 0, 0, 0]),
            Value::Qword(1)
        );
        assert_eq!(decode(REG_QWORD, &[1]), Value::Other);
        assert_eq!(decode(REG_BINARY, &[1, 2]), Value::Binary(vec![1, 2]));
        assert_eq!(decode(0, &[1]), Value::Other);
    }

    /// `<dir>\System32\config\{SOFTWARE,SYSTEM}`, as an offline install has them.
    fn install(name: &str, software: &TestKey, system: &TestKey) -> std::path::PathBuf {
        let windows =
            std::env::temp_dir().join(format!("dpro-offline-{name}-{}", std::process::id()));
        let config = windows.join("System32").join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("SOFTWARE"), build(software)).unwrap();
        std::fs::write(config.join("SYSTEM"), build(system)).unwrap();
        windows
    }

    #[test]
    fn an_offline_install_reads_through_the_view() {
        let current = TestKey::new("CurrentVersion")
            .value(
                "CurrentMajorVersionNumber",
                REG_DWORD,
                &10_u32.to_le_bytes(),
            )
            .value("CurrentMinorVersionNumber", REG_DWORD, &0_u32.to_le_bytes())
            .value("CurrentBuildNumber", REG_SZ, &utf16("26100\0"))
            .value("UBR", REG_DWORD, &7_u32.to_le_bytes());
        let guid = "00112233-4455-6677-8899-aabbccddeeff\0";
        let software = TestKey::new("ROOT").subkey(
            TestKey::new("Microsoft")
                .subkey(TestKey::new("Cryptography").value("MachineGuid", REG_SZ, &utf16(guid)))
                .subkey(TestKey::new("Windows NT").subkey(current)),
        );
        let system = TestKey::new("ROOT")
            .subkey(TestKey::new("ControlSet002"))
            .subkey(TestKey::new("Select").value("Current", REG_DWORD, &2_u32.to_le_bytes()));
        let windows = install("view", &software, &system);

        let registry = OfflineRegistry::load(&windows).unwrap();
        assert!(!registry.is_dirty());
        assert_eq!(registry.control_set(), "ControlSet002");
        assert!(registry.key_exists(Hive::System, "ControlSet002"));
        assert!(!registry.key_exists(Hive::System, "CurrentControlSet"));
        assert!(
            registry
                .value(Hive::Software, "Microsoft", "Nope")
                .is_none()
        );
        let build = identity::os_build(&registry).unwrap();
        assert_eq!(
            (build.major, build.minor, build.build, build.revision),
            (10, 0, 26100, 7)
        );
        assert!(identity::target_id(&registry).is_some());

        let read = offline_software_values(
            &windows,
            r"Microsoft\Windows NT\CurrentVersion",
            &["UBR", "Nope", "CurrentBuildNumber"],
        )
        .unwrap();
        assert_eq!(
            read,
            [
                Some(Value::Dword(7)),
                None,
                Some(Value::Str("26100".into()))
            ]
        );
        let absent = offline_software_values(&windows, r"Microsoft\Nope", &["A", "B"]).unwrap();
        assert_eq!(absent, [None, None]);
        std::fs::remove_dir_all(&windows).unwrap();
    }

    #[test]
    fn a_missing_hive_is_an_error() {
        let windows = std::env::temp_dir().join("dpro-offline-absent");
        assert!(OfflineRegistry::load(&windows).is_err());
    }

    #[test]
    fn control_set_online_and_offline() {
        let online = FakeRegistry::default().with_key(Hive::System, "CurrentControlSet");
        assert_eq!(online.control_set(), "CurrentControlSet");
        let offline =
            FakeRegistry::default().with_value(Hive::System, "Select", "Current", Value::Dword(2));
        assert_eq!(offline.control_set(), "ControlSet002");
    }
}
