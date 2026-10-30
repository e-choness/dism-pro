//! Spike S-07 (roadmap 1.10, TD-0015 §5.7): can DISM Pro track what an installer changes? ETW
//! kernel providers (availability, overhead, attribution including the Windows Installer
//! service), the USN delta and a registry last-write diff, measured on a runner VM. Throwaway
//! measurement code, never shipped.
//!
//!   tracker probe
//!   tracker make-msi <dir> [--files N] [--keys K]
//!   tracker bench fake|msi [--runs R] [--modes none,lean,full] [installer options]
//!   tracker check fake|msi [--profile lean|full] [installer options]
//!   tracker fake-install|fake-child|fake-uninstall [installer options]
//!
//! Installer options: `--files N --keys K` (what it writes), `--root DIR` (where; default
//! `%ProgramFiles%\S07Fake|S07Msi`), `--modify FILE` (fake: an existing file to append to),
//! `--msi FILE` (msi: the package built from `make-msi`'s source).

#![cfg_attr(not(windows), allow(dead_code))]

mod fixture;
mod model;
mod tdh;
#[cfg(windows)]
mod win;

use std::path::Path;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug)]
struct Args {
    command: String,
    /// `fake`/`msi` for bench and check, the output folder for make-msi.
    target: String,
    runs: usize,
    modes: Vec<String>,
    profile: String,
    files: usize,
    keys: usize,
    root: Option<String>,
    modify: Option<String>,
    msi: Option<String>,
}

fn parse_args(raw: &[String]) -> Result<Args> {
    let Some((command, rest)) = raw.split_first() else {
        return Err(
            "usage: tracker <probe|make-msi|bench|check|fake-install|fake-child|fake-uninstall> …"
                .into(),
        );
    };
    let mut a = Args {
        command: command.clone(),
        target: String::new(),
        runs: 5,
        modes: ["none", "lean", "full"].map(String::from).to_vec(),
        profile: "lean".into(),
        files: 1000,
        keys: 50,
        root: None,
        modify: None,
        msi: None,
    };
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        if !arg.starts_with("--") {
            a.target.clone_from(arg);
            continue;
        }
        let value = it.next().ok_or(format!("{arg} needs a value"))?;
        let number = || {
            value
                .parse::<usize>()
                .map_err(|e| format!("{arg} {value}: {e}"))
        };
        match arg.as_str() {
            "--runs" => a.runs = number()?.max(1),
            "--files" => a.files = number()?,
            "--keys" => a.keys = number()?,
            "--modes" => a.modes = value.split(',').map(String::from).collect(),
            "--profile" => a.profile.clone_from(value),
            "--root" => a.root = Some(value.clone()),
            "--modify" => a.modify = Some(value.clone()),
            "--msi" => a.msi = Some(value.clone()),
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    Ok(a)
}

fn run(raw: &[String]) -> Result<String> {
    let a = parse_args(raw)?;
    if a.command == "make-msi" {
        if a.target.is_empty() {
            return Err("make-msi <dir>".into());
        }
        let wxs = fixture::make_msi_source(Path::new(&a.target), a.files, a.keys)?;
        return Ok(format!("{wxs}\n"));
    }
    platform(&a)
}

#[cfg(windows)]
fn platform(a: &Args) -> Result<String> {
    use std::path::PathBuf;
    use win::{Installer, Profile};

    let program_files =
        std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let root = |default: &str| {
        PathBuf::from(
            a.root
                .clone()
                .unwrap_or_else(|| format!(r"{program_files}\{default}")),
        )
    };
    let installer = |kind: &str| -> Result<Installer> {
        match kind {
            "fake" => Ok(Installer::Fake {
                root: root("S07Fake"),
                files: a.files,
                keys: a.keys,
                modify: a.modify.as_ref().map(PathBuf::from),
            }),
            "msi" => Ok(Installer::Msi {
                msi: PathBuf::from(a.msi.as_ref().ok_or("msi needs --msi <package>")?),
                root: root("S07Msi"),
                files: a.files,
                keys: a.keys,
            }),
            other => Err(format!("unknown installer {other:?} (fake|msi)")),
        }
    };
    match a.command.as_str() {
        "probe" => Ok(win::probe()),
        "fake-install" | "fake-child" => {
            let modify = a.modify.as_deref().map(Path::new);
            let child = a.command == "fake-child";
            win::fake_install(&root("S07Fake"), a.files, a.keys, modify, child)?;
            Ok(String::new())
        }
        "fake-uninstall" => {
            installer("fake")?.uninstall()?;
            Ok(String::new())
        }
        "bench" => {
            let modes = a
                .modes
                .iter()
                .map(|m| Profile::parse(m))
                .collect::<Result<Vec<_>>>()?;
            win::bench(&installer(&a.target)?, a.runs, &modes)
        }
        "check" => {
            let profile = Profile::parse(&a.profile)?.ok_or("check needs --profile lean|full")?;
            win::check(&installer(&a.target)?, profile)
        }
        other => Err(format!("unknown command {other}")),
    }
}

#[cfg(not(windows))]
fn platform(a: &Args) -> Result<String> {
    Err(format!("{}: Windows only", a.command))
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match run(&raw) {
        Ok(out) => print!("{out}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
