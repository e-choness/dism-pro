//! Read-only registry access as a trait, so identity and reboot-reason logic is pure and
//! tested with fakes; `dpro-win` backs it on Windows (online HKLM or loaded offline hives).

use std::collections::HashMap;

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

#[cfg(windows)]
pub use win::{OfflineRegistry, OnlineRegistry};

#[cfg(windows)]
mod win {
    use std::path::Path;

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
            RegValue::Binary(_) | RegValue::Other { .. } => Value::Other,
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

    /// An offline installation's hives, loaded read-only as app hives (unloaded on drop).
    #[derive(Debug)]
    pub struct OfflineRegistry {
        software: RegKey,
        system: RegKey,
    }

    impl OfflineRegistry {
        /// `windows_dir` is the absolute Windows folder of the offline install.
        ///
        /// # Errors
        /// A hive file is missing or in use.
        pub fn load(windows_dir: &Path) -> dpro_win::error::Result<Self> {
            let config = windows_dir.join("System32").join("config");
            Ok(Self {
                software: RegKey::load_app_hive(&config.join("SOFTWARE"), Access::Read)?,
                system: RegKey::load_app_hive(&config.join("SYSTEM"), Access::Read)?,
            })
        }
    }

    macro_rules! impl_view {
        ($t:ty) => {
            impl RegistryView for $t {
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
        };
    }

    impl_view!(OnlineRegistry);
    impl_view!(OfflineRegistry);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_set_online_and_offline() {
        let online = FakeRegistry::default().with_key(Hive::System, "CurrentControlSet");
        assert_eq!(online.control_set(), "CurrentControlSet");
        let offline =
            FakeRegistry::default().with_value(Hive::System, "Select", "Current", Value::Dword(2));
        assert_eq!(offline.control_set(), "ControlSet002");
    }
}
