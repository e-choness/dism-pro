//! `cargo xtask bench-gate <summaries.jsonl> [--update]`: the deterministic gate (doc 04 §3).
//!
//! Reads the JSON summaries gungraun prints with `--output-format=json` (one object per
//! benchmark, schema v7) and compares each benchmark's Callgrind instruction count (Ir) with
//! `benches/baseline.json`. The run fails when a count is more than [`LIMIT_PERCENT`] over its
//! baseline, when a benchmark has no baseline, and when a baseline has no benchmark. With
//! `--update` it writes the baseline from the run instead; the PR says why.
//!
//! `docker compose run --rm bench-gate` runs the benchmarks first (docker/task.sh).

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Write as _},
    fs,
    path::Path,
};

use serde_json::{Map, Value};

use crate::Result;

/// Doc 04 §3: an instruction-count regression above 5 % fails CI.
const LIMIT_PERCENT: u64 = 5;
const BASELINE: &str = "benches/baseline.json";
const ABOUT: &str = "Callgrind instruction counts (Ir) of the gungraun benches, the deterministic \
gate of doc 04 §3: CI fails a count more than 5 % over its entry here. Rewrite with \
`docker compose run --rm bench-gate -- --update` and say why in the PR.";

/// Instruction counts by benchmark name, sorted.
type Counts = BTreeMap<String, u64>;

pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    let (summaries, update) = match args {
        [file] => (file, false),
        [file, flag] if flag == "--update" => (file, true),
        _ => return Err("usage: cargo xtask bench-gate <summaries.jsonl> [--update]".into()),
    };
    let text = fs::read_to_string(root.join(summaries))
        .map_err(|e| format!("{summaries}: {e} (run the benches first: docker/task.sh)"))?;
    let counts = parse_summaries(&text).map_err(|e| format!("{summaries}: {e}"))?;
    let baseline_path = root.join(BASELINE);

    if update {
        if let Some(dir) = baseline_path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&baseline_path, render_baseline(&counts)?)
            .map_err(|e| format!("{BASELINE}: {e}"))?;
        for (name, ir) in &counts {
            println!("bench-gate: {ir:>13} Ir  {name}");
        }
        println!("bench-gate: wrote {BASELINE} ({} benches)", counts.len());
        return Ok(());
    }

    let baseline_text = fs::read_to_string(&baseline_path).map_err(|e| {
        format!("{BASELINE}: {e} (create it with `docker compose run --rm bench-gate -- --update`)")
    })?;
    let baseline = parse_baseline(&baseline_text).map_err(|e| format!("{BASELINE}: {e}"))?;
    let report = compare(&baseline, &counts);
    for line in &report.lines {
        println!("bench-gate: {line}");
    }
    for name in &report.faster {
        crate::notice(
            "bench-gate",
            &format!(
                "{name} runs more than {LIMIT_PERCENT} % fewer instructions than {BASELINE}: lower \
                 its baseline with `docker compose run --rm bench-gate -- --update`"
            ),
        );
    }
    if report.failures.is_empty() {
        println!(
            "bench-gate: {} benches within +{LIMIT_PERCENT} % of {BASELINE}",
            counts.len()
        );
        return Ok(());
    }
    Err(report.failures.join("\n").into())
}

/// One benchmark against its baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Ok,
    /// More than the limit under its baseline: worth lowering the baseline.
    Faster,
    Regressed,
    NoBaseline,
    Missing,
}

#[derive(Debug, Default)]
struct Report {
    lines: Vec<String>,
    failures: Vec<String>,
    faster: Vec<String>,
}

fn compare(baseline: &Counts, counts: &Counts) -> Report {
    let mut report = Report::default();
    let names: BTreeSet<&String> = baseline.keys().chain(counts.keys()).collect();
    for name in names {
        let (verdict, line) = match (baseline.get(name), counts.get(name)) {
            (Some(&old), Some(&new)) => {
                let verdict = verdict(old, new);
                let change = Change { old, new };
                if verdict == Verdict::Regressed {
                    report.failures.push(format!(
                        "{name}: {change} ({new} Ir, baseline {old}), over the +{LIMIT_PERCENT} % \
                         limit"
                    ));
                }
                let line = format!(
                    "{:<9} {change:>9} {new:>13} Ir (baseline {old}) {name}",
                    label(verdict)
                );
                (verdict, line)
            }
            (None, Some(&new)) => (
                Verdict::NoBaseline,
                format!(
                    "{:<9} {:>9} {new:>13} Ir {name}",
                    label(Verdict::NoBaseline),
                    ""
                ),
            ),
            (Some(&old), None) => (
                Verdict::Missing,
                format!(
                    "{:<9} {:>9} {:>13}    (baseline {old}) {name}",
                    label(Verdict::Missing),
                    "",
                    "-"
                ),
            ),
            (None, None) => continue,
        };
        match verdict {
            // A regression's failure is pushed above, where its numbers are.
            Verdict::Ok | Verdict::Regressed => {}
            Verdict::Faster => report.faster.push(name.clone()),
            Verdict::NoBaseline => report.failures.push(format!(
                "{name}: no baseline in {BASELINE}; add it with `bench-gate -- --update`"
            )),
            Verdict::Missing => report.failures.push(format!(
                "{name}: in {BASELINE} but didn't run; remove it with `bench-gate -- --update`"
            )),
        }
        report.lines.push(line);
    }
    report
}

/// Regressed when `new > old × (100 + limit) / 100`, faster when `new < old × (100 − limit) / 100`.
fn verdict(old: u64, new: u64) -> Verdict {
    let (old, new) = (u128::from(old), u128::from(new));
    let limit = u128::from(LIMIT_PERCENT);
    if new * 100 > old * (100 + limit) {
        Verdict::Regressed
    } else if new * 100 < old * (100 - limit) {
        Verdict::Faster
    } else {
        Verdict::Ok
    }
}

fn label(v: Verdict) -> &'static str {
    match v {
        Verdict::Ok => "ok",
        Verdict::Faster => "faster",
        Verdict::Regressed => "REGRESSED",
        Verdict::NoBaseline => "NEW",
        Verdict::Missing => "MISSING",
    }
}

/// The change from `old` to `new` in percent, to two decimals: `+1.25 %`.
struct Change {
    old: u64,
    new: u64,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (old, new) = (i128::from(self.old), i128::from(self.new));
        let text = if old == 0 {
            if new == 0 {
                "+0.00 %".to_owned()
            } else {
                "+inf %".to_owned()
            }
        } else {
            // Hundredths of a percent, rounded half away from zero.
            let diff = (new - old) * 10_000;
            let hundredths = (diff + diff.signum() * old / 2) / old;
            let sign = if hundredths < 0 { '-' } else { '+' };
            let abs = hundredths.unsigned_abs();
            format!("{sign}{}.{:02} %", abs / 100, abs % 100)
        };
        f.pad(&text)
    }
}

/// Instruction counts from gungraun's JSON summaries: one object per benchmark, whitespace
/// between them.
fn parse_summaries(text: &str) -> std::result::Result<Counts, String> {
    let mut counts = Counts::new();
    for (i, value) in serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .enumerate()
    {
        let value = value.map_err(|e| format!("summary {}: {e}", i + 1))?;
        let (name, ir) = summary_count(&value).map_err(|e| format!("summary {}: {e}", i + 1))?;
        if counts.insert(name.clone(), ir).is_some() {
            return Err(format!("{name} appears twice"));
        }
    }
    if counts.is_empty() {
        return Err("no benchmark summaries (did the `ir` benches run?)".into());
    }
    Ok(counts)
}

/// `(name, Ir)` of one summary. The name is `<bench file> <group>::<function>.<id>`, which is
/// stable across runs and unique in the workspace.
fn summary_count(v: &Value) -> std::result::Result<(String, u64), String> {
    let field = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("no string `{key}`"))
    };
    if field("version")? != "7" {
        return Err(format!(
            "summary schema {} (this gate reads v7, gungraun 0.20)",
            field("version")?
        ));
    }
    let mut name = format!(
        "{} {}::{}",
        field("benchmark_file")?,
        field("group")?,
        field("function_name")?
    );
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        name.push('.');
        name.push_str(id);
    }
    let ir = v
        .get("profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| {
            profiles
                .iter()
                .find(|p| p.get("tool").and_then(Value::as_str) == Some("Callgrind"))
        })
        .and_then(|p| p.pointer("/data/total/metrics/Ir/values/new"))
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{name}: no Callgrind Ir count"))?;
    Ok((name, ir))
}

fn parse_baseline(text: &str) -> std::result::Result<Counts, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let entries = v
        .get("instructions")
        .and_then(Value::as_object)
        .ok_or("no `instructions` object")?;
    entries
        .iter()
        .map(|(name, ir)| {
            ir.as_u64()
                .map(|ir| (name.clone(), ir))
                .ok_or_else(|| format!("{name}: not a count"))
        })
        .collect()
}

fn render_baseline(counts: &Counts) -> Result<String> {
    let instructions: Map<String, Value> = counts
        .iter()
        .map(|(name, &ir)| (name.clone(), Value::from(ir)))
        .collect();
    let mut doc = Map::new();
    doc.insert("about".into(), Value::from(ABOUT));
    doc.insert("instructions".into(), Value::Object(instructions));
    let mut out = serde_json::to_string_pretty(&Value::Object(doc))?;
    writeln!(out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gungraun 0.20 summary as printed by `--output-format=json`, trimmed to what matters.
    fn summary(function: &str, id: &str, ir: u64) -> String {
        format!(
            r#"{{"baselines":[null,null],"benchmark_file":"crates/dpro-app/benches/ir.rs","function_name":"{function}","group":"palette_1000","id":"{id}","kind":"LibraryBenchmark","profiles":[{{"data":{{"parts":[{{"metrics":{{"Ir":{{"values":{{"new":1}}}}}}}}],"total":{{"metrics":{{"Ir":{{"values":{{"new":{ir}}}}}}},"regressions":[]}}}},"tool":"Callgrind"}}],"version":"7"}}"#
        )
    }

    fn counts(entries: &[(&str, u64)]) -> Counts {
        entries.iter().map(|&(n, c)| (n.to_owned(), c)).collect()
    }

    #[test]
    fn summaries_give_one_named_total_per_benchmark() {
        let text = format!(
            "{}\n{}\n",
            summary("keystroke", "empty", 203_567),
            summary("keystroke", "r", 1_414_175)
        );
        assert_eq!(
            parse_summaries(&text).unwrap(),
            counts(&[
                (
                    "crates/dpro-app/benches/ir.rs palette_1000::keystroke.empty",
                    203_567
                ),
                (
                    "crates/dpro-app/benches/ir.rs palette_1000::keystroke.r",
                    1_414_175
                ),
            ])
        );
    }

    #[test]
    fn bad_summaries_are_errors() {
        assert!(parse_summaries("").is_err());
        let twice = format!("{0}{0}", summary("keystroke", "r", 1));
        assert!(parse_summaries(&twice).unwrap_err().contains("twice"));
        let v6 = summary("keystroke", "r", 1).replace(r#""version":"7""#, r#""version":"6""#);
        assert!(parse_summaries(&v6).unwrap_err().contains("schema 6"));
        let no_ir = summary("keystroke", "r", 1).replace("total", "sum");
        assert!(
            parse_summaries(&no_ir)
                .unwrap_err()
                .contains("no Callgrind Ir")
        );
    }

    #[test]
    fn verdicts_use_the_limit_inclusively() {
        assert_eq!(verdict(1_000, 1_050), Verdict::Ok);
        assert_eq!(verdict(1_000, 1_051), Verdict::Regressed);
        assert_eq!(verdict(1_000, 950), Verdict::Ok);
        assert_eq!(verdict(1_000, 949), Verdict::Faster);
        assert_eq!(verdict(u64::MAX, u64::MAX), Verdict::Ok);
    }

    #[test]
    fn compare_fails_regressions_new_and_missing_benches() {
        let baseline = counts(&[("a", 100), ("b", 100), ("gone", 7), ("quick", 100)]);
        let now = counts(&[("a", 104), ("b", 106), ("new", 9), ("quick", 50)]);
        let report = compare(&baseline, &now);
        assert_eq!(report.lines.len(), 5);
        assert_eq!(report.faster, ["quick"]);
        let failed: Vec<&str> = report
            .failures
            .iter()
            .map(|f| f.split(':').next().unwrap_or_default())
            .collect();
        assert_eq!(failed, ["b", "gone", "new"]);
    }

    #[test]
    fn changes_print_to_two_decimals() {
        let c = |old, new| Change { old, new }.to_string();
        assert_eq!(c(1_000, 1_000), "+0.00 %");
        assert_eq!(c(3, 4), "+33.33 %");
        assert_eq!(c(3, 2), "-33.33 %");
        assert_eq!(c(200_000, 200_001), "+0.00 %");
        assert_eq!(c(0, 5), "+inf %");
    }

    #[test]
    fn baselines_round_trip() {
        let c = counts(&[("x y::z.a", 1), ("x y::z.b", 2)]);
        let text = render_baseline(&c).unwrap();
        assert!(text.ends_with("}\n"));
        assert_eq!(parse_baseline(&text).unwrap(), c);
        assert!(parse_baseline(r#"{"instructions":{"a":-1}}"#).is_err());
        assert!(parse_baseline("{}").is_err());
    }
}
