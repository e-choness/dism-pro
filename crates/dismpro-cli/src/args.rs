//! Command-line parsing (TD-0002 §4, §5.1). Pure: no I/O, tested in C1.

use std::{ffi::OsString, path::PathBuf};

use dpro_core::id::JobId;
use dpro_mcp::catalog::Toolset;
use lexopt::{Arg, Parser, ValueExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetSel {
    Online,
    /// A volume root or offline Windows root (`D:`, `D:\`, `E:\mnt`).
    Root(PathBuf),
    /// A mounted image directory.
    Image(PathBuf),
}

/// What a mutating command does with its plan (TD-0002 §4). `--dry-run` and `--yes` are
/// mutually exclusive, so they're one setting rather than two flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ApplyMode {
    /// Print the plan, then ask on a console or stop with exit 3.
    #[default]
    Ask,
    /// `--dry-run`: print the plan and stop.
    DryRun,
    /// `--yes`: apply without asking (permanent changes still need `--allow-irreversible`).
    Yes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Global {
    pub(crate) target: TargetSel,
    pub(crate) json: bool,
    pub(crate) mode: ApplyMode,
    pub(crate) allow_irreversible: bool,
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) limit_access: bool,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            target: TargetSel::Online,
            json: false,
            mode: ApplyMode::Ask,
            allow_irreversible: false,
            sources: Vec::new(),
            limit_access: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help,
    Version,
    TargetsList,
    PackagesList,
    PackagesRemove(Vec<String>),
    FeaturesList,
    FeaturesSet {
        enable: bool,
        names: Vec<String>,
    },
    Health {
        scan: bool,
    },
    HealthRestore,
    RebootReasons,
    JournalList,
    JournalShow(JournalRef),
    JournalUndo(JournalRef),
    Purge,
    /// The MCP server on stdio (TD-0013), optionally narrowed to some toolsets.
    Mcp {
        toolsets: Option<Vec<Toolset>>,
    },
    /// The elevated agent broker for one user's fronts (internal; TD-0013 §15).
    AgentBroker {
        user_sid: String,
    },
}

/// A journaled job: by id (looked up in the target's journal folder) or by file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JournalRef {
    Id(JobId),
    File(PathBuf),
}

impl JournalRef {
    fn parse(s: &str) -> Self {
        s.parse()
            .map_or_else(|_| Self::File(PathBuf::from(s)), Self::Id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cli {
    pub(crate) global: Global,
    pub(crate) command: Command,
}

/// A usage error, already worded for `cli-error-usage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageError(pub(crate) String);

impl From<lexopt::Error> for UsageError {
    fn from(e: lexopt::Error) -> Self {
        Self(e.to_string())
    }
}

/// Parses arguments **without** the program name.
pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Cli, UsageError> {
    let mut p = Parser::from_args(args);
    let mut g = Global::default();
    let mut words: Vec<String> = Vec::new();
    let mut help = false;
    let mut version = false;
    let mut dry_run = false;
    let mut yes = false;
    let mut toolsets: Option<String> = None;
    let mut agent_broker = false;
    let mut user_sid: Option<String> = None;
    while let Some(arg) = p.next()? {
        match arg {
            Arg::Short('h') | Arg::Long("help") => help = true,
            Arg::Short('V') | Arg::Long("version") => version = true,
            Arg::Long("json") => g.json = true,
            Arg::Long("dry-run") => dry_run = true,
            Arg::Long("yes") | Arg::Short('y') => yes = true,
            Arg::Long("allow-irreversible") => g.allow_irreversible = true,
            Arg::Long("limit-access") => g.limit_access = true,
            Arg::Long("source") => g.sources.push(PathBuf::from(p.value()?)),
            Arg::Long("target") => g.target = target(&p.value()?.string()?)?,
            Arg::Long("toolsets") => toolsets = Some(p.value()?.string()?),
            // Internal: how the MCP front starts its elevated broker (TD-0013 §15).
            Arg::Long("agent-broker") => agent_broker = true,
            Arg::Long("user-sid") => user_sid = Some(p.value()?.string()?),
            Arg::Value(v) => words.push(v.string()?),
            other => return Err(other.unexpected().into()),
        }
    }
    g.mode = match (dry_run, yes) {
        (true, true) => return Err(UsageError("--dry-run and --yes can't be combined".into())),
        (true, false) => ApplyMode::DryRun,
        (false, true) => ApplyMode::Yes,
        (false, false) => ApplyMode::Ask,
    };
    let mut command = if help {
        Command::Help
    } else if version {
        Command::Version
    } else if agent_broker {
        if !words.is_empty() {
            return Err(UsageError("--agent-broker takes no command".into()));
        }
        let sid = user_sid
            .take()
            .ok_or_else(|| UsageError("--agent-broker needs --user-sid".into()))?;
        Command::AgentBroker { user_sid: sid }
    } else {
        command(&words)?
    };
    if user_sid.is_some() {
        return Err(UsageError(
            "--user-sid goes only with --agent-broker".into(),
        ));
    }
    if let Some(list) = toolsets {
        let Command::Mcp { toolsets } = &mut command else {
            return Err(UsageError("--toolsets goes only with mcp".into()));
        };
        *toolsets = Some(parse_toolsets(&list)?);
    }
    Ok(Cli { global: g, command })
}

/// `core,maintain`: the toolsets an MCP client's configuration asks for (TD-0013 §10).
fn parse_toolsets(list: &str) -> Result<Vec<Toolset>, UsageError> {
    let mut sets = Vec::new();
    for name in list.split(',').map(str::trim) {
        let set = Toolset::parse(name).ok_or_else(|| {
            let known: Vec<&str> = Toolset::ALL.iter().map(|t| t.name()).collect();
            UsageError(format!(
                "unknown toolset {name:?} (known: {})",
                known.join(", ")
            ))
        })?;
        if !sets.contains(&set) {
            sets.push(set);
        }
    }
    Ok(sets)
}

pub(crate) fn target(s: &str) -> Result<TargetSel, UsageError> {
    if s.eq_ignore_ascii_case("online") {
        return Ok(TargetSel::Online);
    }
    if let Some(dir) = s.strip_prefix("image:") {
        return Ok(TargetSel::Image(PathBuf::from(dir)));
    }
    let bytes = s.as_bytes();
    let is_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if is_drive {
        // `D:` alone means the volume root, not the current directory on D:.
        let root = if s.len() == 2 {
            format!("{s}\\")
        } else {
            s.to_owned()
        };
        return Ok(TargetSel::Root(PathBuf::from(root)));
    }
    Err(UsageError(format!(
        "invalid --target {s:?} (use online, D:, a path like D:\\ or image:<dir>)"
    )))
}

fn command(words: &[String]) -> Result<Command, UsageError> {
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    let rest = |n: usize| words[n..].to_vec();
    let need = |n: usize, what: &str| {
        if words.len() > n {
            Ok(())
        } else {
            Err(UsageError(format!("{what} needs at least one name")))
        }
    };
    Ok(match w.as_slice() {
        [] => Command::Help,
        ["targets", "list"] | ["targets"] => Command::TargetsList,
        ["packages", "list"] | ["packages"] => Command::PackagesList,
        ["packages", "remove", ..] => {
            need(2, "packages remove")?;
            Command::PackagesRemove(rest(2))
        }
        ["features", "list"] | ["features"] => Command::FeaturesList,
        ["features", verb @ ("enable" | "disable"), ..] => {
            need(2, "features enable/disable")?;
            Command::FeaturesSet {
                enable: *verb == "enable",
                names: rest(2),
            }
        }
        ["health", "check"] => Command::Health { scan: false },
        ["health", "scan"] => Command::Health { scan: true },
        ["health", "restore"] => Command::HealthRestore,
        ["reboot-reasons"] => Command::RebootReasons,
        ["journal", "list"] | ["journal"] => Command::JournalList,
        ["journal", "show", job] => Command::JournalShow(JournalRef::parse(job)),
        ["journal", "undo", job] => Command::JournalUndo(JournalRef::parse(job)),
        ["purge"] => Command::Purge,
        ["mcp"] => Command::Mcp { toolsets: None },
        _ => return Err(UsageError(format!("unknown command {:?}", words.join(" ")))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Cli, UsageError> {
        parse(s.split_whitespace().map(OsString::from))
    }

    #[test]
    fn commands() {
        let cases: &[(&str, Command)] = &[
            ("", Command::Help),
            ("--help", Command::Help),
            ("targets list", Command::TargetsList),
            ("--version", Command::Version),
            ("packages", Command::PackagesList),
            (
                "packages remove A B",
                Command::PackagesRemove(vec!["A".into(), "B".into()]),
            ),
            (
                "features disable SMB1Protocol",
                Command::FeaturesSet {
                    enable: false,
                    names: vec!["SMB1Protocol".into()],
                },
            ),
            (
                "features enable X",
                Command::FeaturesSet {
                    enable: true,
                    names: vec!["X".into()],
                },
            ),
            ("health check", Command::Health { scan: false }),
            ("health scan", Command::Health { scan: true }),
            ("health restore", Command::HealthRestore),
            ("reboot-reasons", Command::RebootReasons),
            ("journal list", Command::JournalList),
            (
                "journal show j.dpj",
                Command::JournalShow(JournalRef::File("j.dpj".into())),
            ),
            (
                "journal undo 01JBCDEFGHJKMNPQRSTVWXYZ01",
                Command::JournalUndo(JournalRef::Id(
                    "01JBCDEFGHJKMNPQRSTVWXYZ01".parse().unwrap(),
                )),
            ),
            ("purge", Command::Purge),
            ("mcp", Command::Mcp { toolsets: None }),
            (
                "mcp --toolsets maintain,core,maintain",
                Command::Mcp {
                    toolsets: Some(vec![Toolset::Maintain, Toolset::Core]),
                },
            ),
            (
                "--agent-broker --user-sid S-1-5-21-1-2-3-1001",
                Command::AgentBroker {
                    user_sid: "S-1-5-21-1-2-3-1001".into(),
                },
            ),
        ];
        for (args, expected) in cases {
            assert_eq!(&p(args).unwrap().command, expected, "{args}");
        }
    }

    #[test]
    fn globals_anywhere() {
        let cli = p("features --json disable --target D: X --dry-run").unwrap();
        assert!(cli.global.json);
        assert_eq!(cli.global.mode, ApplyMode::DryRun);
        assert_eq!(p("-y purge").unwrap().global.mode, ApplyMode::Yes);
        assert_eq!(p("purge").unwrap().global.mode, ApplyMode::Ask);
        assert_eq!(cli.global.target, TargetSel::Root(PathBuf::from("D:\\")));
        let cli = p("--target image:C:\\mnt --source E:\\sources\\install.wim --limit-access health restore").unwrap();
        assert_eq!(
            cli.global.target,
            TargetSel::Image(PathBuf::from("C:\\mnt"))
        );
        assert_eq!(
            cli.global.sources,
            [PathBuf::from("E:\\sources\\install.wim")]
        );
        assert!(cli.global.limit_access);
    }

    #[test]
    fn usage_errors() {
        for bad in [
            "packages remove",
            "features enable",
            "frobnicate",
            "--target nowhere targets",
            "--color never targets",
            "--bogus targets",
            "--dry-run --yes features disable X",
            "journal show",
            "mcp --toolsets",
            "mcp --toolsets core,bogus",
            "mcp --toolsets ,",
            "targets --toolsets core",
            "--agent-broker",
            "--agent-broker mcp --user-sid S-1-5-18",
            "mcp --user-sid S-1-5-18",
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }
}
