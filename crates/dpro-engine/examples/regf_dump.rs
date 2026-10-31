//! Throwaway cross-check (diag/t3-regf only): dumps a hive in a canonical form that
//! `.github/scripts/regf-check.ps1` also produces through `offreg.dll`.
//!
//!   regf_dump dump <hive> <out>   every key and value
//!   regf_dump time <hive>         reads CurrentVersion\CurrentBuild once, prints microseconds

#![allow(
    missing_docs,
    clippy::print_stdout,
    clippy::unwrap_used,
    reason = "throwaway"
)]

use std::{
    env, fs,
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

use dpro_engine::regf::{Key, Regf};

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn dump(out: &mut impl Write, key: Key<'_>, path: &str) {
    let mut subkeys: Vec<_> = key
        .subkeys()
        .unwrap()
        .into_iter()
        .map(|k| (k.name().unwrap(), k))
        .collect();
    subkeys.sort_by_key(|(name, _)| units(name));
    let mut values: Vec<_> = key
        .values()
        .unwrap()
        .into_iter()
        .map(|v| (v.name().unwrap(), v))
        .collect();
    values.sort_by_key(|(name, _)| units(name));
    writeln!(
        out,
        "K\t{path}\t{}\t{}\t{}",
        key.last_written().unwrap(),
        subkeys.len(),
        values.len()
    )
    .unwrap();
    for (name, value) in values {
        let data = value.data().unwrap();
        writeln!(
            out,
            "V\t{name}\t{}\t{}\t{:016x}",
            value.kind().unwrap(),
            data.len(),
            fnv(&data)
        )
        .unwrap();
    }
    for (name, sub) in subkeys {
        dump(out, sub, &format!("{path}\\{name}"));
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("dump") => {
            let hive = Regf::open(Path::new(&args[1])).unwrap();
            let mut out = BufWriter::new(fs::File::create(&args[2]).unwrap());
            dump(&mut out, hive.root().unwrap(), "");
            out.flush().unwrap();
            println!("dirty={}", hive.is_dirty());
        }
        Some("time") => {
            let t = Instant::now();
            let hive = Regf::open(Path::new(&args[1])).unwrap();
            let key = hive
                .key(r"Microsoft\Windows NT\CurrentVersion")
                .unwrap()
                .unwrap();
            let value = key.value("CurrentBuild").unwrap().unwrap();
            let data = value.data().unwrap();
            let us = t.elapsed().as_micros();
            println!("{us} {}", data.len());
        }
        _ => println!("regf_dump dump|time <hive> [out]"),
    }
}
