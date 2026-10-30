//! Protected-path denylist (TD-0001 §8). Changes to these paths are refused whatever the
//! rule pack or agent asks for; the TI helper re-checks the same list.
//!
//! Paths are matched like Windows does: case-insensitive (ASCII), `\` or `/`, with the
//! `\\?\` and drive prefixes ignored and on whole components only. Callers pass the path
//! as seen from the target's root (for an offline target, relative to its volume root).

/// Protected locations, root-relative, lower-case, `\`-separated. Everything below a
/// directory entry is protected too.
const PROTECTED: &[&str] = &[
    // Registry hives.
    r"windows\system32\config",
    // Component store: file-level changes only via the DISM API.
    r"windows\winsxs",
    // Paging, hibernation and swap files.
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
    // Boot files.
    "bootmgr",
    "bootnxt",
    "boot",
    "efi",
    r"windows\boot",
    // BitLocker and other volume metadata.
    "system volume information",
];

/// `true` if a change to `path` (or anything under it) must be refused. Paths that
/// aren't canonical (`.`/`..` components) are refused too: canonicalize first.
#[must_use]
pub fn is_protected(path: &str) -> bool {
    let Some(components) = normalize(path) else {
        return true;
    };
    PROTECTED.iter().any(|entry| {
        let protected: Vec<&str> = entry.split('\\').collect();
        components.len() >= protected.len()
            && components.iter().zip(&protected).all(|(c, p)| c == p)
    })
}

/// Lower-cased components below the root, or `None` for `.`/`..` components.
fn normalize(path: &str) -> Option<Vec<String>> {
    let path = path.replace('/', "\\");
    let path = path
        .strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix(r"\\.\"))
        .unwrap_or(&path);
    let bytes = path.as_bytes();
    let path = if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        &path[2..]
    } else {
        path
    };
    let mut out = Vec::new();
    for c in path.split('\\').filter(|c| !c.is_empty()) {
        if c == "." || c == ".." {
            return None;
        }
        out.push(c.to_ascii_lowercase());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_listed_locations_in_any_spelling() {
        for p in [
            r"C:\Windows\System32\config\SAM",
            r"\\?\C:\WINDOWS\system32\CONFIG",
            "c:/windows/winsxs/amd64_foo/bar.dll",
            r"D:\pagefile.sys",
            r"\Boot\BCD",
            r"C:\System Volume Information\FVE2.{e40ad34d-dae9-4bc7-95bd-b16218c10f72}",
            r"C:\Windows\..\Windows\System32\drivers",
        ] {
            assert!(is_protected(p), "{p}");
        }
    }

    #[test]
    fn allows_neighbours_and_lookalikes() {
        for p in [
            r"C:\Windows\Temp\x.log",
            r"C:\Windows\System32\configuration.txt",
            r"C:\Windows\WinSxS.bak",
            r"C:\Users\me\boot\notes.txt",
            r"C:\pagefile.sys.old",
        ] {
            assert!(!is_protected(p), "{p}");
        }
    }
}
