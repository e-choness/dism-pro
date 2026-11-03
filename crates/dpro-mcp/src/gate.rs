//! The checks a call passes before its tool runs (TD-0013 §4.4, §15), in this order:
//! 1. the tool exists (else a protocol error);
//! 2. the arguments fit its input schema;
//! 3. the policy allows the tool;
//! 4. the policy allows the target it names.
//!
//! The front runs them so a bad or forbidden call never brings up a UAC prompt. The broker runs
//! them again, on the policy as it is at that moment, and its answer is the one that counts.

use serde_json::Value;

use crate::{
    catalog::Tool,
    policy::{Policy, TargetKind},
    tool::CallError,
};

/// Longest string argument accepted, in bytes. Arguments are names, paths, ids and a one-line
/// reason; anything longer is a mistake or an attack (TD-0013 §8.1: length caps).
pub const MAX_STRING: usize = 4096;

/// A call that passed: its tool, and the target it names (`online` when it names none).
#[derive(Debug, Clone, Copy)]
pub struct Allowed<'a> {
    pub tool: &'a Tool,
    pub target: TargetKind<'a>,
}

/// Checks one call. `policy` is `None` when the front can't read the policy: it then checks only
/// the tool and its arguments, and the broker decides the rest.
///
/// # Errors
/// - [`CallError::UnknownTool`];
/// - [`CallError::Invalid`] for arguments that don't fit the schema or a target that doesn't parse;
/// - [`CallError::Failed`] starting `policy_forbids: ` when the policy says no.
pub fn check<'a>(
    tools: &'a [Tool],
    policy: Option<&Policy>,
    name: &str,
    args: &'a Value,
) -> Result<Allowed<'a>, CallError> {
    let tool = tools
        .iter()
        .find(|t| t.def.name == name)
        .ok_or_else(|| CallError::UnknownTool(name.to_owned()))?;
    validate(&tool.def.input_schema, args).map_err(CallError::Invalid)?;
    let selector = args.get("target").and_then(Value::as_str);
    let target = selector
        .map_or(Ok(TargetKind::Online), parse_target)
        .map_err(CallError::Invalid)?;
    if let Some(policy) = policy {
        let forbids =
            |e: crate::policy::Forbidden| CallError::Failed(format!("policy_forbids: {e}"));
        policy.check_tool(tool).map_err(forbids)?;
        policy
            .check_target(target, selector.unwrap_or("online"))
            .map_err(forbids)?;
    }
    Ok(Allowed { tool, target })
}

/// A target selector as tools take it, the CLI's `--target` forms: `online`, a drive (`D:`,
/// `D:\`), a folder holding Windows (`E:\mnt`), or `image:<mount folder>`.
///
/// # Errors
/// What's wrong with it, for the agent.
pub fn parse_target(s: &str) -> Result<TargetKind<'_>, String> {
    if s.eq_ignore_ascii_case("online") {
        return Ok(TargetKind::Online);
    }
    if let Some(dir) = s.strip_prefix("image:") {
        return if is_absolute(dir) {
            Ok(TargetKind::Image(dir))
        } else {
            Err(format!(
                "image:{dir} needs a full path, like image:C:\\mount"
            ))
        };
    }
    if is_absolute(s) {
        return Ok(TargetKind::Offline(s));
    }
    Err(format!(
        "target {s:?} isn't one of: online, a drive like D:, a folder like D:\\ or image:<folder>. \
         targets_list shows the valid ones."
    ))
}

/// `D:` or `D:\…`: a drive-letter path. Relative and UNC paths aren't targets.
fn is_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'\\' || b[2] == b'/')
}

/// Checks arguments against the subset of JSON Schema the catalog uses: a closed object of
/// strings, booleans, bounded integers and arrays of strings. A missing `arguments` (null) is
/// an empty object.
///
/// # Errors
/// What's wrong, naming the argument.
pub fn validate(schema: &Value, args: &Value) -> Result<(), String> {
    let empty = serde_json::Map::new();
    let object = match args {
        Value::Null => &empty,
        Value::Object(o) => o,
        _ => return Err("arguments must be an object".to_owned()),
    };
    let properties = schema.get("properties").and_then(Value::as_object);
    for (key, value) in object {
        let Some(property) = properties.and_then(|p| p.get(key)) else {
            return Err(format!("unknown argument {key:?}"));
        };
        check_value(key, property, value)?;
    }
    for required in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !object.contains_key(required) {
            return Err(format!("{required} is required"));
        }
    }
    Ok(())
}

fn check_value(key: &str, property: &Value, value: &Value) -> Result<(), String> {
    let want = property.get("type").and_then(Value::as_str).unwrap_or("");
    let fits = match want {
        "string" => match value.as_str() {
            Some(s) if s.len() > MAX_STRING => {
                return Err(format!("{key} is longer than {MAX_STRING} bytes"));
            }
            Some(_) => true,
            None => false,
        },
        "boolean" => value.is_boolean(),
        "integer" => {
            let Some(n) = value.as_i64() else {
                return Err(format!("{key} must be an integer"));
            };
            let min = property.get("minimum").and_then(Value::as_i64);
            let max = property.get("maximum").and_then(Value::as_i64);
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                return Err(format!(
                    "{key} must be between {} and {}",
                    min.unwrap_or(i64::MIN),
                    max.unwrap_or(i64::MAX)
                ));
            }
            true
        }
        "array" => {
            let Some(items) = value.as_array() else {
                return Err(format!("{key} must be an array"));
            };
            let item = property.get("items").unwrap_or(&Value::Null);
            for (i, v) in items.iter().enumerate() {
                check_value(&format!("{key}[{i}]"), item, v)?;
            }
            true
        }
        // The catalog has no other types; an unknown one is a catalog bug, so refuse.
        _ => return Err(format!("{key} has a type this server can't check")),
    };
    if fits {
        Ok(())
    } else {
        Err(format!("{key} must be a {want}"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::catalog::{self, Tier, Toolset};

    fn tools() -> Vec<Tool> {
        catalog::tools()
    }

    #[test]
    fn arguments_must_fit_the_schema() {
        let t = tools();
        let ok = |name: &str, args: Value| check(&t, None, name, &args).map(|_| ());
        assert!(ok("targets_list", Value::Null).is_ok());
        assert!(ok("targets_list", json!({})).is_ok());
        assert!(ok("journal_list", json!({ "limit": 500, "fields": ["job"] })).is_ok());
        let invalid = |name: &str, args: Value| match ok(name, args) {
            Err(CallError::Invalid(m)) => m,
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(
            invalid("targets_list", json!({ "x": 1 })),
            "unknown argument \"x\""
        );
        assert_eq!(
            invalid("targets_list", json!([1])),
            "arguments must be an object"
        );
        assert_eq!(invalid("plan_get", json!({})), "plan_id is required");
        assert_eq!(
            invalid("plan_get", json!({ "plan_id": 7 })),
            "plan_id must be a string"
        );
        assert_eq!(
            invalid("journal_list", json!({ "limit": 0 })),
            "limit must be between 1 and 500"
        );
        assert_eq!(
            invalid("journal_list", json!({ "limit": 1.5 })),
            "limit must be an integer"
        );
        assert_eq!(
            invalid("journal_list", json!({ "fields": ["a", 2] })),
            "fields[1] must be a string"
        );
        assert_eq!(
            invalid("health_repair_plan", json!({ "limit_access": "yes" })),
            "limit_access must be a boolean"
        );
        let long = "x".repeat(MAX_STRING + 1);
        assert!(invalid("plan_get", json!({ "plan_id": long })).contains("longer than"));
    }

    #[test]
    fn unknown_tools_are_protocol_errors() {
        let t = tools();
        assert!(matches!(
            check(&t, None, "format_c", &Value::Null),
            Err(CallError::UnknownTool(n)) if n == "format_c"
        ));
    }

    #[test]
    fn targets_parse_like_the_cli() {
        assert_eq!(parse_target("ONLINE"), Ok(TargetKind::Online));
        assert_eq!(parse_target("D:"), Ok(TargetKind::Offline("D:")));
        assert_eq!(parse_target("E:\\mnt"), Ok(TargetKind::Offline("E:\\mnt")));
        assert_eq!(
            parse_target("image:C:\\mount"),
            Ok(TargetKind::Image("C:\\mount"))
        );
        for bad in [
            "",
            "D",
            "relative\\dir",
            "\\\\server\\share",
            "image:",
            "image:mnt",
            "DE:",
        ] {
            assert!(parse_target(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_policy_gates_tools_and_targets() {
        let t = tools();
        let policy = Policy::default(); // plan tier, online only
        let forbidden = |name: &str, args: Value| match check(&t, Some(&policy), name, &args) {
            Err(CallError::Failed(m)) => {
                assert!(m.starts_with("policy_forbids: "), "{m}");
                m
            }
            other => panic!("{name}: {other:?}"),
        };
        let m = forbidden("health_check", json!({ "target": "D:" }));
        assert!(m.contains("target D:"), "{m}");
        let m = forbidden(
            "plan_apply",
            json!({ "plan_id": "p", "fingerprint": "f", "reason": "r" }),
        );
        assert!(m.contains("apply_reversible"), "{m}");
        let none = json!({});
        let allowed = check(&t, Some(&policy), "health_check", &none).unwrap();
        assert_eq!(allowed.target, TargetKind::Online);
        assert_eq!(allowed.tool.def.name, "health_check");

        let offline = Policy {
            tier: Tier::Observe,
            toolsets: vec![Toolset::Maintain],
            targets: vec![crate::policy::TargetRule::AnyOffline],
            ..Policy::default()
        };
        let d = json!({ "target": "d:\\" });
        let allowed = check(&t, Some(&offline), "health_check", &d).unwrap();
        assert_eq!(allowed.target, TargetKind::Offline("d:\\"));
        // Without the policy (the front couldn't read it) only the tool and arguments count.
        assert!(
            check(
                &t,
                None,
                "plan_apply",
                &json!({ "plan_id": "p", "fingerprint": "f", "reason": "r" })
            )
            .is_ok()
        );
        // Arguments are checked before the policy: a typo isn't reported as forbidden.
        assert!(matches!(
            check(
                &t,
                Some(&offline),
                "targets_list",
                &json!({ "bogus": true })
            ),
            Err(CallError::Invalid(_))
        ));
    }
}
