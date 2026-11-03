//! The agent policy (TD-0013 §5.7): which tier, toolsets and targets a human allows agents.
//!
//! It lives in `%ProgramData%\DismPro\agent-policy.toml`: everyone may read it, only
//! administrators may change it, and no tool writes it. The broker reads it on every call, so a
//! change applies at once. The file is the flat subset of TOML the GUI writes:
//!
//! ```toml
//! schema = 1
//! tier = "plan"                 # observe | plan | apply_reversible | apply_with_approval
//! toolsets = ["core", "maintain"]
//! targets = ["online"]          # "online", "offline:*", "offline:D:", "image:*", "image:<dir>"
//! max_changes_per_plan = 5000
//! approval_timeout_minutes = 10
//! redact_user_paths = true
//! allow_network_tools = false
//! ```
//!
//! One `key = value` per line; strings, integers, booleans and one-line arrays of strings;
//! `#` comments. Anything else, an unknown key or a repeated one is an error naming the line:
//! callers then refuse every tool (fail closed) rather than guess. A missing file means
//! [`Policy::default`].

use crate::catalog::{Tier, Tool, Toolset};

/// Defaults: owner decision #11 (Plan only) and TD-0013 §5.7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub tier: Tier,
    pub toolsets: Vec<Toolset>,
    pub targets: Vec<TargetRule>,
    pub max_changes_per_plan: u32,
    pub approval_timeout_minutes: u32,
    pub redact_user_paths: bool,
    pub allow_network_tools: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            tier: Tier::Plan,
            toolsets: Toolset::DEFAULT.to_vec(),
            targets: vec![TargetRule::Online],
            max_changes_per_plan: 5000,
            approval_timeout_minutes: 10,
            redact_user_paths: true,
            allow_network_tools: false,
        }
    }
}

/// Which targets agents may name. Offline selectors compare case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetRule {
    Online,
    AnyOffline,
    /// An offline Windows by its CLI selector (`D:`).
    Offline(String),
    AnyImage,
    /// A mounted image by its folder.
    Image(String),
}

/// A target as the broker resolved it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind<'a> {
    Online,
    Offline(&'a str),
    Image(&'a str),
}

/// Why a call isn't allowed. The messages are for the agent to relay to the human, who can
/// change the policy in DISM Pro (TD-0013 §4.4 `policy_forbids`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Forbidden {
    #[error("the agent policy's tier is {have}; {tool} needs {needed}. The user can change it in DISM Pro: Settings, Agent access.", have = .have.name(), needed = .needed.name())]
    Tier {
        tool: &'static str,
        have: Tier,
        needed: Tier,
    },
    #[error("the agent policy doesn't enable the {toolset} toolset, which {tool} is in.", toolset = .toolset.name())]
    Toolset {
        tool: &'static str,
        toolset: Toolset,
    },
    #[error(
        "the agent policy doesn't allow target {0}. The user can allow it in DISM Pro: Settings, Agent access."
    )]
    Target(String),
}

impl Policy {
    /// May this policy call `tool`?
    ///
    /// # Errors
    /// [`Forbidden`] with what's missing.
    pub fn check_tool(&self, tool: &Tool) -> Result<(), Forbidden> {
        if !self.toolsets.contains(&tool.toolset) {
            return Err(Forbidden::Toolset {
                tool: tool.def.name,
                toolset: tool.toolset,
            });
        }
        if self.tier < tool.tier {
            return Err(Forbidden::Tier {
                tool: tool.def.name,
                have: self.tier,
                needed: tool.tier,
            });
        }
        Ok(())
    }

    /// May agents name this target? `shown` is how the error names it.
    ///
    /// # Errors
    /// [`Forbidden::Target`].
    pub fn check_target(&self, target: TargetKind<'_>, shown: &str) -> Result<(), Forbidden> {
        let allowed = self.targets.iter().any(|rule| match (rule, target) {
            (TargetRule::Online, TargetKind::Online)
            | (TargetRule::AnyOffline, TargetKind::Offline(_))
            | (TargetRule::AnyImage, TargetKind::Image(_)) => true,
            (TargetRule::Offline(want), TargetKind::Offline(have))
            | (TargetRule::Image(want), TargetKind::Image(have)) => same_selector(want, have),
            _ => false,
        });
        if allowed {
            Ok(())
        } else {
            Err(Forbidden::Target(shown.to_owned()))
        }
    }
}

/// `D:` and `D:\` name the same drive; case doesn't matter on Windows.
fn same_selector(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("agent policy, line {line}: {message}")]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

/// Parses a policy file.
///
/// # Errors
/// [`ParseError`] for anything outside the subset above, an unknown or repeated key, a wrong
/// type, an unknown tier, toolset or target rule, or a `schema` other than 1.
pub fn parse(text: &str) -> Result<Policy, ParseError> {
    let mut policy = Policy::default();
    let mut seen: Vec<String> = Vec::new();
    let mut schema = None;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let err = |message: String| ParseError { line, message };
        let mut cursor = Cursor::new(raw);
        cursor.skip_space();
        if cursor.at_end_or_comment() {
            continue;
        }
        let key = cursor.key().map_err(err)?;
        if seen.contains(&key) {
            return Err(err(format!("{key} is set twice")));
        }
        cursor.skip_space();
        cursor.expect('=').map_err(err)?;
        cursor.skip_space();
        let value = cursor.value().map_err(err)?;
        cursor.skip_space();
        if !cursor.at_end_or_comment() {
            return Err(err("unexpected text after the value".to_owned()));
        }
        apply(&mut policy, &mut schema, &key, value).map_err(err)?;
        seen.push(key);
    }
    match schema {
        Some(1) => Ok(policy),
        Some(n) => Err(ParseError {
            line: 0,
            message: format!("schema {n} isn't one this version reads (1)"),
        }),
        None => Err(ParseError {
            line: 0,
            message: "schema = 1 is missing".to_owned(),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    List(Vec<String>),
}

fn apply(
    policy: &mut Policy,
    schema: &mut Option<i64>,
    key: &str,
    value: Value,
) -> Result<(), String> {
    match (key, value) {
        ("schema", Value::Int(n)) => *schema = Some(n),
        ("tier", Value::Str(s)) => {
            policy.tier = Tier::parse(&s).ok_or_else(|| format!("unknown tier {s:?}"))?;
        }
        ("toolsets", Value::List(names)) => {
            policy.toolsets = names
                .iter()
                .map(|n| Toolset::parse(n).ok_or_else(|| format!("unknown toolset {n:?}")))
                .collect::<Result<_, _>>()?;
        }
        ("targets", Value::List(rules)) => {
            policy.targets = rules
                .iter()
                .map(|r| target_rule(r))
                .collect::<Result<_, _>>()?;
        }
        ("max_changes_per_plan", Value::Int(n)) => policy.max_changes_per_plan = positive(n)?,
        ("approval_timeout_minutes", Value::Int(n)) => {
            policy.approval_timeout_minutes = positive(n)?;
        }
        ("redact_user_paths", Value::Bool(b)) => policy.redact_user_paths = b,
        ("allow_network_tools", Value::Bool(b)) => policy.allow_network_tools = b,
        (
            "schema"
            | "tier"
            | "toolsets"
            | "targets"
            | "max_changes_per_plan"
            | "approval_timeout_minutes"
            | "redact_user_paths"
            | "allow_network_tools",
            _,
        ) => return Err(format!("{key} has the wrong type")),
        _ => return Err(format!("unknown key {key}")),
    }
    Ok(())
}

fn target_rule(text: &str) -> Result<TargetRule, String> {
    let rule = match text {
        "online" => TargetRule::Online,
        "offline:*" => TargetRule::AnyOffline,
        "image:*" => TargetRule::AnyImage,
        _ => match (text.strip_prefix("offline:"), text.strip_prefix("image:")) {
            (Some(sel), _) if !sel.is_empty() => TargetRule::Offline(sel.to_owned()),
            (_, Some(dir)) if !dir.is_empty() => TargetRule::Image(dir.to_owned()),
            _ => return Err(format!("unknown target rule {text:?}")),
        },
    };
    Ok(rule)
}

fn positive(n: i64) -> Result<u32, String> {
    u32::try_from(n)
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("{n} isn't a positive whole number"))
}

/// A position in one line.
struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    fn new(line: &'a str) -> Self {
        Self { rest: line }
    }

    fn skip_space(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\t']);
    }

    fn at_end_or_comment(&self) -> bool {
        self.rest.is_empty() || self.rest.starts_with('#') || self.rest == "\r"
    }

    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.rest = &self.rest[c.len_utf8()..];
        Some(c)
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        match self.bump() {
            Some(c) if c == want => Ok(()),
            Some(c) => Err(format!("expected {want:?}, found {c:?}")),
            None => Err(format!("expected {want:?} at the end of the line")),
        }
    }

    /// A bare key: letters, digits, `_` and `-`.
    fn key(&mut self) -> Result<String, String> {
        let len = self
            .rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(self.rest.len());
        if len == 0 {
            return Err("expected a key".to_owned());
        }
        let (key, rest) = self.rest.split_at(len);
        self.rest = rest;
        Ok(key.to_owned())
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some('"' | '\'') => Ok(Value::Str(self.string()?)),
            Some('[') => self.list(),
            Some(_) => self.scalar(),
            None => Err("expected a value".to_owned()),
        }
    }

    fn scalar(&mut self) -> Result<Value, String> {
        let len = self
            .rest
            .find(|c: char| c.is_whitespace() || c == '#')
            .unwrap_or(self.rest.len());
        let (word, rest) = self.rest.split_at(len);
        self.rest = rest;
        match word {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => word
                .replace('_', "")
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| format!("{word:?} isn't a string, number, boolean or list")),
        }
    }

    /// `"basic"` with `\"`, `\\`, `\t`, `\n` escapes, or `'literal'`.
    fn string(&mut self) -> Result<String, String> {
        let quote = self.bump().ok_or("expected a string")?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err("a string isn't closed on its line".to_owned()),
                Some(c) if c == quote => return Ok(out),
                Some('\\') if quote == '"' => match self.bump() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('t') => out.push('\t'),
                    Some('n') => out.push('\n'),
                    other => return Err(format!("unsupported escape \\{}", other.unwrap_or(' '))),
                },
                Some(c) if c.is_control() && c != '\t' => {
                    return Err("control character in a string".to_owned());
                }
                Some(c) => out.push(c),
            }
        }
    }

    /// `["a", "b"]` on one line, trailing comma allowed.
    fn list(&mut self) -> Result<Value, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some(']') => {
                    self.bump();
                    return Ok(Value::List(items));
                }
                Some('"' | '\'') => items.push(self.string()?),
                _ => return Err("a list holds strings and ends on its line with ]".to_owned()),
            }
            self.skip_space();
            match self.bump() {
                Some(',') => {}
                Some(']') => return Ok(Value::List(items)),
                _ => return Err("expected , or ] in a list".to_owned()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tools;

    const FULL: &str = r#"
# DISM Pro agent policy
schema = 1
tier = "observe"                    # observe | plan | apply_reversible | apply_with_approval
toolsets = ["core", "maintain",]
targets = ["online", "offline:D:", 'image:C:\mnt']
max_changes_per_plan = 1_000
approval_timeout_minutes = 5
redact_user_paths = false
allow_network_tools = true
"#;

    #[test]
    fn reads_every_key() {
        let p = parse(FULL).unwrap();
        assert_eq!(p.tier, Tier::Observe);
        assert_eq!(p.toolsets, vec![Toolset::Core, Toolset::Maintain]);
        assert_eq!(
            p.targets,
            vec![
                TargetRule::Online,
                TargetRule::Offline("D:".into()),
                TargetRule::Image(r"C:\mnt".into())
            ]
        );
        assert_eq!(p.max_changes_per_plan, 1000);
        assert_eq!(p.approval_timeout_minutes, 5);
        assert!(!p.redact_user_paths);
        assert!(p.allow_network_tools);
    }

    #[test]
    fn missing_keys_keep_their_defaults() {
        let p = parse("schema = 1\r\ntier = 'plan'\r\n").unwrap();
        assert_eq!(p, Policy::default());
    }

    #[test]
    fn errors_name_the_line() {
        for (text, line, needle) in [
            ("schema = 1\nteir = \"plan\"", 2, "unknown key teir"),
            ("schema = 1\ntier = \"godmode\"", 2, "unknown tier"),
            ("schema = 1\ntier = 3", 2, "wrong type"),
            (
                "schema = 1\ntoolsets = [\"core\",\n\"maintain\"]",
                2,
                "ends on its line",
            ),
            (
                "schema = 1\ntier = \"plan\"\ntier = \"observe\"",
                3,
                "set twice",
            ),
            (
                "schema = 1\ntargets = [\"offline:\"]",
                2,
                "unknown target rule",
            ),
            ("schema = 1\nmax_changes_per_plan = 0", 2, "positive"),
            ("schema = 1\ntier = \"plan\" extra", 2, "after the value"),
            ("schema = 1\n[table]", 2, "expected a key"),
            ("schema = 1\ntier = \"open", 2, "isn't closed"),
        ] {
            let e = parse(text).unwrap_err();
            assert_eq!(e.line, line, "{text:?}: {e}");
            assert!(e.message.contains(needle), "{text:?}: {e}");
        }
    }

    #[test]
    fn the_schema_line_is_required_and_must_be_one() {
        assert!(
            parse("tier = \"plan\"")
                .unwrap_err()
                .message
                .contains("missing")
        );
        assert!(
            parse("schema = 2")
                .unwrap_err()
                .message
                .contains("schema 2")
        );
    }

    #[test]
    fn tiers_gate_tools() {
        let observe = Policy {
            tier: Tier::Observe,
            ..Policy::default()
        };
        let plan = Policy::default();
        for tool in tools() {
            let need = tool.tier;
            assert_eq!(observe.check_tool(&tool).is_ok(), need == Tier::Observe);
            assert_eq!(plan.check_tool(&tool).is_ok(), need <= Tier::Plan);
            if need > Tier::Plan {
                let e = plan.check_tool(&tool).unwrap_err();
                assert!(e.to_string().contains("Settings, Agent access"), "{e}");
            }
        }
    }

    #[test]
    fn toolsets_gate_tools() {
        let core_only = Policy {
            toolsets: vec![Toolset::Core],
            ..Policy::default()
        };
        let health = tools()
            .into_iter()
            .find(|t| t.def.name == "health_check")
            .unwrap();
        assert!(matches!(
            core_only.check_tool(&health),
            Err(Forbidden::Toolset { .. })
        ));
    }

    #[test]
    fn target_rules() {
        let p = parse(FULL).unwrap();
        assert!(p.check_target(TargetKind::Online, "online").is_ok());
        assert!(p.check_target(TargetKind::Offline("d:\\"), "d:\\").is_ok());
        assert!(p.check_target(TargetKind::Offline("E:"), "E:").is_err());
        assert!(
            p.check_target(TargetKind::Image(r"c:\MNT"), r"image:c:\MNT")
                .is_ok()
        );
        let default = Policy::default();
        assert!(default.check_target(TargetKind::Online, "online").is_ok());
        assert!(matches!(
            default.check_target(TargetKind::Offline("D:"), "D:"),
            Err(Forbidden::Target(t)) if t == "D:"
        ));
        let any = parse("schema = 1\ntargets = [\"offline:*\", \"image:*\"]").unwrap();
        assert!(any.check_target(TargetKind::Offline("Z:"), "Z:").is_ok());
        assert!(any.check_target(TargetKind::Image(r"X:\y"), "img").is_ok());
        assert!(any.check_target(TargetKind::Online, "online").is_err());
    }
}

#[cfg(test)]
mod properties {
    use proptest::prelude::*;

    use super::parse;

    proptest! {
        /// The elevated broker reads this file: no input may panic it.
        #[test]
        fn never_panics(text in r"\PC{0,400}") {
            let _ = parse(&text);
        }

        #[test]
        fn never_panics_on_policy_like_lines(
            lines in proptest::collection::vec(
                r#"(schema|tier|toolsets|targets|max_changes_per_plan|[a-z_]{1,8}) ?= ?("[^"\n]{0,12}"?|'[^'\n]{0,12}'?|\[["', a-z:*\\]{0,20}\]?|-?[0-9_]{1,12}|true|false)? ?(#.*)?"#,
                0..8,
            )
        ) {
            let _ = parse(&lines.join("\n"));
        }
    }
}
