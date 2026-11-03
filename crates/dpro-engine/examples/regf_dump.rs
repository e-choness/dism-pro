//! Dumps a hive with [`dpro_engine::regf`] in the canonical form that
//! `.github/scripts/regf-check.ps1` also writes through `offreg.dll`, so that T3 can compare the
//! reader with the OS on the offline image's real hives (ADR-0008). A dev tool, never shipped.
//!
//! ```text
//! regf_dump dump <hive> <out>   every key and value, one line each
//! regf_dump time <hive>         reads CurrentVersion\CurrentBuild once, prints microseconds
//! ```
//!
//! Key lines are `K<TAB>path<TAB>last written<TAB>subkeys<TAB>values` and value lines
//! `V<TAB>name<TAB>type<TAB>size<TAB>FNV-1a of the data`. Subkeys and values sort by UTF-16 code
//! unit, as .NET's `string.CompareOrdinal` does.

use std::{
    env, fmt,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
    process::ExitCode,
    time::Instant,
};

use dpro_engine::regf::{Key, Regf, RegfError};

/// The OS allows 512 levels; a deeper walk is a cycle in a damaged hive.
const MAX_DEPTH: usize = 512;

const USAGE: &str = "usage: regf_dump dump <hive> <out> | regf_dump time <hive>";

#[derive(Debug)]
enum Error {
    Usage,
    Hive(RegfError),
    Io(io::Error),
    TooDeep(String),
    NoBuild,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => f.write_str(USAGE),
            Self::Hive(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::TooDeep(path) => write!(f, "deeper than {MAX_DEPTH} levels at {path}"),
            Self::NoBuild => f.write_str(r"no Microsoft\Windows NT\CurrentVersion\CurrentBuild"),
        }
    }
}

impl From<RegfError> for Error {
    fn from(e: RegfError) -> Self {
        Self::Hive(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn dump(out: &mut impl Write, key: &Key<'_>, path: &str, depth: usize) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::TooDeep(path.to_owned()));
    }
    let mut subkeys = key
        .subkeys()?
        .into_iter()
        .map(|k| Ok((k.name()?, k)))
        .collect::<Result<Vec<_>, RegfError>>()?;
    subkeys.sort_by_cached_key(|(name, _)| units(name));
    let mut values = key
        .values()?
        .into_iter()
        .map(|v| Ok((v.name()?, v)))
        .collect::<Result<Vec<_>, RegfError>>()?;
    values.sort_by_cached_key(|(name, _)| units(name));
    writeln!(
        out,
        "K\t{path}\t{}\t{}\t{}",
        key.last_written()?,
        subkeys.len(),
        values.len()
    )?;
    for (name, value) in values {
        let data = value.data()?;
        writeln!(
            out,
            "V\t{name}\t{}\t{}\t{:016x}",
            value.kind()?,
            data.len(),
            fnv1a(&data)
        )?;
    }
    for (name, sub) in subkeys {
        dump(out, &sub, &format!("{path}\\{name}"), depth + 1)?;
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), Error> {
    let mut stdout = io::stdout().lock();
    match args {
        [command, hive, out] if command == "dump" => {
            let hive = Regf::open(Path::new(hive))?;
            let mut out = BufWriter::new(File::create(out)?);
            dump(&mut out, &hive.root()?, "", 0)?;
            out.flush()?;
            writeln!(stdout, "dirty={}", hive.is_dirty())?;
        }
        [command, hive] if command == "time" => {
            let t = Instant::now();
            let hive = Regf::open(Path::new(hive))?;
            let data = hive
                .key(r"Microsoft\Windows NT\CurrentVersion")?
                .and_then(|key| key.value("CurrentBuild").transpose())
                .transpose()?
                .ok_or(Error::NoBuild)?
                .data()?;
            let us = t.elapsed().as_micros();
            writeln!(stdout, "{us} {}", data.len())?;
        }
        _ => return Err(Error::Usage),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // ignore: nothing is left to report a failed write to stderr to.
            let _ = writeln!(io::stderr(), "regf_dump: {e}");
            ExitCode::FAILURE
        }
    }
}
