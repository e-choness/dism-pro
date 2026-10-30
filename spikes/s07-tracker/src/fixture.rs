//! The two test installers: what each writes (the ground truth) and the `WiX` source of the MSI.
//! Both are deterministic, so the bench recomputes the truth instead of passing it around.
//!
//! - **fake**: `tracker fake-install` starts a child and exits at once; the child writes the
//!   files, the registry keys, a value in an existing deep key (`Run`), an uninstall entry, and
//!   appends to an existing file. Tests job tracking past the parent's exit.
//! - **msi**: the same files and keys in a per-machine MSI, so the Windows Installer service
//!   does the writing, outside the job (TD-0015 §5.4/§5.7).

use std::{fmt::Write as _, fs, path::Path};

use crate::{Result, model::Truth};

pub const MSI_PRODUCT_CODE: &str = "{5D07A5E1-0C5B-4A0E-9C1D-7A2F3B4C5D61}";
pub const MSI_UPGRADE_CODE: &str = "{5D07A5E1-0C5B-4A0E-9C1D-7A2F3B4C5D62}";
const MSI_REGISTRY_COMPONENT: &str = "{5D07A5E1-0C5B-4A0E-9C1D-7A2F3B4C5D63}";
pub const RUN_KEY: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
pub const ARP_KEY: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
pub const FAKE_ARP_NAME: &str = "S07Fake";
pub const FAKE_RUN_VALUE: &str = "S07Fake";
pub const MSI_RUN_VALUE: &str = "S07Msi";

/// `d<i mod 10>\f<i>.bin` below the install folder.
pub fn file_rel(i: usize) -> String {
    format!(r"d{}\f{i:05}.bin", i % 10)
}

pub fn file_size(i: usize) -> usize {
    (i * 37) % 4096 + 1
}

/// `HKLM\SOFTWARE\S07Test\<kind>`: the parent of an installer's numbered keys.
pub fn data_key(kind: &str) -> String {
    format!(r"HKLM\SOFTWARE\S07Test\{kind}")
}

pub fn numbered_key(kind: &str, j: usize) -> String {
    format!(r"{}\k{j:04}", data_key(kind))
}

/// Strips `HKLM\` for the Win32 calls that take a subkey of `HKEY_LOCAL_MACHINE`.
pub fn hklm_subkey(key: &str) -> &str {
    key.strip_prefix(r"HKLM\").unwrap_or(key)
}

fn common(kind: &str, dir: &Path, files: usize, keys: usize) -> Truth {
    let dir = dir.display().to_string();
    let keys: Vec<String> = (0..keys).map(|j| numbered_key(kind, j)).collect();
    Truth {
        files: (0..files)
            .map(|i| format!(r"{dir}\{}", file_rel(i)))
            .collect(),
        modified: Vec::new(),
        values: keys
            .iter()
            .flat_map(|k| [(k.clone(), "v".into()), (k.clone(), "n".into())])
            .collect(),
        keys,
    }
}

pub fn fake_truth(dir: &Path, files: usize, keys: usize, modify: Option<&Path>) -> Truth {
    let mut t = common("fake", dir, files, keys);
    t.keys.push(data_key("fake"));
    let arp = format!(r"{ARP_KEY}\{FAKE_ARP_NAME}");
    t.values.push((RUN_KEY.into(), FAKE_RUN_VALUE.into()));
    t.values.push((arp.clone(), "DisplayName".into()));
    t.keys.push(arp);
    t.modified.extend(modify.map(|m| m.display().to_string()));
    t
}

pub fn msi_truth(dir: &Path, files: usize, keys: usize) -> Truth {
    let mut t = common("msi", dir, files, keys);
    let arp = format!(r"{ARP_KEY}\{MSI_PRODUCT_CODE}");
    t.values.push((RUN_KEY.into(), MSI_RUN_VALUE.into()));
    t.values.push((arp.clone(), "DisplayName".into()));
    t.keys.push(arp);
    t
}

/// Writes `payload\…` and `s07.wxs` (`WiX` v4+ schema) into `out`; returns the .wxs path.
pub fn make_msi_source(out: &Path, files: usize, keys: usize) -> Result<String> {
    let payload = std::path::absolute(out.join("payload")).map_err(|e| e.to_string())?;
    for i in 0..files {
        let path = payload.join(file_rel(i));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, vec![b'm'; file_size(i)])
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let wxs_path = out.join("s07.wxs");
    fs::write(&wxs_path, wxs(&payload.display().to_string(), files, keys))
        .map_err(|e| format!("{}: {e}", wxs_path.display()))?;
    Ok(wxs_path.display().to_string())
}

fn wxs(payload: &str, files: usize, keys: usize) -> String {
    let mut w = String::new();
    let _ = writeln!(w, r#"<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">"#);
    let _ = writeln!(
        w,
        r#"  <Package Name="S07 spike test" Manufacturer="DISM Pro spike S-07" Version="1.0.0.0" UpgradeCode="{MSI_UPGRADE_CODE}" ProductCode="{MSI_PRODUCT_CODE}" Scope="perMachine">"#
    );
    let _ = writeln!(w, r#"    <MediaTemplate EmbedCab="yes" />"#);
    let _ = writeln!(w, r#"    <StandardDirectory Id="ProgramFiles64Folder">"#);
    let _ = writeln!(w, r#"      <Directory Id="INSTALLFOLDER" Name="S07Msi">"#);
    for d in 0..files.min(10) {
        let _ = writeln!(w, r#"        <Directory Id="dir{d}" Name="d{d}">"#);
        for i in (d..files).step_by(10) {
            let _ = writeln!(
                w,
                r#"          <Component Id="c{i:05}"><File Id="f{i:05}" Source="{payload}\{}" /></Component>"#,
                file_rel(i)
            );
        }
        let _ = writeln!(w, "        </Directory>");
    }
    let _ = writeln!(
        w,
        r#"        <Component Id="reg" Guid="{MSI_REGISTRY_COMPONENT}">"#
    );
    for j in 0..keys {
        let key = hklm_subkey(&numbered_key("msi", j)).to_owned();
        let keypath = if j == 0 { r#" KeyPath="yes""# } else { "" };
        let _ = writeln!(
            w,
            r#"          <RegistryValue Root="HKLM" Key="{key}" Name="v" Type="string" Value="x"{keypath} />"#
        );
        let _ = writeln!(
            w,
            r#"          <RegistryValue Root="HKLM" Key="{key}" Name="n" Type="integer" Value="{j}" />"#
        );
    }
    let _ = writeln!(
        w,
        r#"          <RegistryValue Root="HKLM" Key="{}" Name="{MSI_RUN_VALUE}" Type="string" Value="[INSTALLFOLDER]d0\f00000.bin" />"#,
        hklm_subkey(RUN_KEY)
    );
    let _ = writeln!(w, "        </Component>");
    let _ = writeln!(w, "      </Directory>");
    let _ = writeln!(w, "    </StandardDirectory>");
    let _ = writeln!(w, r#"    <Feature Id="Main">"#);
    for i in 0..files {
        let _ = writeln!(w, r#"      <ComponentRef Id="c{i:05}" />"#);
    }
    let _ = writeln!(w, r#"      <ComponentRef Id="reg" />"#);
    let _ = writeln!(w, "    </Feature>");
    let _ = writeln!(w, "  </Package>");
    let _ = writeln!(w, "</Wix>");
    w
}

#[cfg(test)]
mod tests {
    use super::{fake_truth, msi_truth, wxs};
    use std::path::Path;

    #[test]
    fn truth_counts() {
        let t = fake_truth(
            Path::new(r"C:\Program Files\S07Fake"),
            25,
            3,
            Some(Path::new(r"C:\x.ini")),
        );
        assert_eq!(
            (
                t.files.len(),
                t.keys.len(),
                t.values.len(),
                t.modified.len()
            ),
            (25, 5, 8, 1)
        );
        assert_eq!(t.files[13], r"C:\Program Files\S07Fake\d3\f00013.bin");
        let t = msi_truth(Path::new(r"C:\Program Files\S07Msi"), 25, 3);
        assert_eq!((t.files.len(), t.keys.len(), t.values.len()), (25, 4, 8));
    }

    #[test]
    fn wxs_lists_every_component() {
        let w = wxs(r"D:\s07\payload", 25, 3);
        assert_eq!(w.matches("<Component Id=\"c").count(), 25);
        assert_eq!(w.matches("<ComponentRef ").count(), 26);
        assert_eq!(w.matches("KeyPath=\"yes\"").count(), 1);
        assert!(w.contains(r#"Source="D:\s07\payload\d3\f00013.bin""#));
        assert!(w.contains(r#"Key="SOFTWARE\S07Test\msi\k0002""#));
    }
}
