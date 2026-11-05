//! The tools DISM Pro offers agents (TD-0013 §5.3), as far as roadmap 2.7 goes: the `core` and
//! `maintain` toolsets with what the engine can do today. Tools for jobs, cleanup and Windows
//! Update join when their engines exist.
//!
//! Rules from TD-0013 §5.3: domain-specific tools, and no tool mutates. A change is a `*_plan`
//! (read-only, stored by the broker) followed by `plan_apply`, which needs an apply tier
//! (roadmap 3.5). Descriptions are short: the default catalog must stay under 3,500 tokens
//! (TD-0013 §7, `tests/token_budgets.rs`).

use serde_json::{Map, Value, json};

use crate::tool::{Annotations, ToolDef};

/// Groups of tools a policy enables (TD-0013 §5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Toolset {
    Core,
    Maintain,
}

impl Toolset {
    pub const ALL: [Self; 2] = [Self::Core, Self::Maintain];
    /// What a policy without a `toolsets` line enables.
    pub const DEFAULT: [Self; 2] = [Self::Core, Self::Maintain];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Maintain => "maintain",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// Policy tiers, lowest first (TD-0013 §4.2). The human picks one; a tool needs at least its
/// own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Observe,
    Plan,
    ApplyReversible,
    ApplyWithApproval,
}

impl Tier {
    pub const ALL: [Self; 4] = [
        Self::Observe,
        Self::Plan,
        Self::ApplyReversible,
        Self::ApplyWithApproval,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Plan => "plan",
            Self::ApplyReversible => "apply_reversible",
            Self::ApplyWithApproval => "apply_with_approval",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// Where a tool runs: pure tools in the unelevated front, the rest in the elevated broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    Front,
    Broker,
}

/// One catalog entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub def: ToolDef,
    pub toolset: Toolset,
    /// The lowest policy tier that may call it.
    pub tier: Tier,
    pub runs: Runs,
}

/// Every tool, in the order `tools/list` shows them.
#[must_use]
pub fn tools() -> Vec<Tool> {
    let mut all = core();
    all.extend(maintain());
    all
}

fn core() -> Vec<Tool> {
    use Runs::{Broker, Front};
    use Tier::{Observe, Plan};
    use Toolset::Core;
    vec![
        tool(
            Core,
            Observe,
            Broker,
            "targets_list",
            "List targets",
            "Windows installations DISM Pro can service: the running one and offline ones on \
             other drives. Use a `selector` as the `target` of other tools.",
            no_args(),
        ),
        tool(
            Core,
            Observe,
            Broker,
            "system_overview",
            "System overview",
            "Build, edition, component-store health (quick check), pending restart and its \
             reasons, and free space for one target. Start here.",
            object(&[("target", target())], &[]),
        ),
        tool(
            Core,
            Observe,
            Broker,
            "plan_get",
            "Get plan",
            "The changes of a plan made by a *_plan tool, one page at a time.",
            object(
                &[
                    ("plan_id", string("From the *_plan result.")),
                    ("limit", limit()),
                    ("cursor", cursor()),
                ],
                &["plan_id"],
            ),
        ),
        plan_apply(),
        tool(
            Core,
            Observe,
            Broker,
            "journal_list",
            "List journal",
            "Jobs applied to a target, newest first, with status and whether they can be undone.",
            list_args(&[("target", target())]),
        ),
        tool(
            Core,
            Plan,
            Broker,
            "undo_plan",
            "Plan undo",
            "Plan the undo of a journaled job. Nothing changes until plan_apply.",
            object(
                &[
                    ("job_id", string("From journal_list.")),
                    ("target", target()),
                ],
                &["job_id"],
            ),
        ),
        tool(
            Core,
            Observe,
            Front,
            "explain_error",
            "Explain error",
            "What an HRESULT, Win32 or CBS/DISM error code means and what to try next.",
            object(
                &[("code", string("Hex like 0x800f081f, or decimal."))],
                &["code"],
            ),
        ),
    ]
}

fn maintain() -> Vec<Tool> {
    use Runs::Broker;
    use Tier::{Observe, Plan};
    use Toolset::Maintain;
    vec![
        tool(
            Maintain,
            Observe,
            Broker,
            "health_check",
            "Check health",
            "Quick component-store check (seconds): healthy, repairable or not repairable. \
             Findings come from the logs it writes.",
            object(&[("target", target())], &[]),
        ),
        tool(
            Maintain,
            Observe,
            Broker,
            "health_diagnose",
            "Diagnose health",
            "Findings in the target's recent CBS log (errors, missing sources, corruption), \
             each with a next step.",
            list_args(&[("target", target())]),
        ),
        tool(
            Maintain,
            Plan,
            Broker,
            "health_repair_plan",
            "Plan repair",
            "Plan a component-store repair (RestoreHealth). Permanent; may download from \
             Windows Update unless limit_access.",
            object(
                &[
                    ("target", target()),
                    ("limit_access", boolean("Don't contact Windows Update.")),
                    (
                        "sources",
                        json!({ "type": "array", "items": { "type": "string" },
                                "description": "Repair sources: install.wim, a mounted image's Windows folder." }),
                    ),
                ],
                &[],
            ),
        ),
        tool(
            Maintain,
            Observe,
            Broker,
            "updates_list",
            "List installed updates",
            "Installed packages and updates of a target. as_of is when DISM read the list: the \
             server keeps it until the target is serviced again, for at most 10 minutes.",
            list_args(&[("target", target())]),
        ),
    ]
}

/// The one destructive tool: it asks; a human approves (TD-0013 §4.3).
fn plan_apply() -> Tool {
    use Runs::Broker;
    use Tier::ApplyReversible;
    use Toolset::Core;
    Tool {
        def: ToolDef {
            annotations: Annotations {
                read_only: false,
                destructive: true,
                idempotent: false,
                open_world: false,
            },
            ..def(
                "plan_apply",
                "Apply plan",
                "Ask to apply a plan. A human approves it in DISM Pro unless policy allows \
             otherwise; show the user the plan summary, including what is permanent, \
             before calling.",
                object(
                    &[
                        ("plan_id", string("From the *_plan result.")),
                        (
                            "fingerprint",
                            string("From the same result; a stale plan is refused."),
                        ),
                        (
                            "reason",
                            string("Why, in a sentence; shown to the human as unverified."),
                        ),
                    ],
                    &["plan_id", "fingerprint", "reason"],
                ),
            )
        },
        toolset: Core,
        tier: ApplyReversible,
        runs: Broker,
    }
}

/// The tools a policy shows: every tool of the enabled toolsets, in catalog order. Tools above
/// the policy's tier stay listed; calling one says which tier it needs (TD-0013 §4.4).
#[must_use]
pub fn enabled(toolsets: &[Toolset]) -> Vec<Tool> {
    tools()
        .into_iter()
        .filter(|t| toolsets.contains(&t.toolset))
        .collect()
}

fn tool(
    toolset: Toolset,
    tier: Tier,
    runs: Runs,
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
) -> Tool {
    Tool {
        def: def(name, title, description, input_schema),
        toolset,
        tier,
        runs,
    }
}

fn def(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
) -> ToolDef {
    ToolDef {
        name,
        title,
        description,
        input_schema,
        output_schema: None,
        annotations: Annotations::READ_ONLY,
    }
}

fn object(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let properties: Map<String, Value> = properties
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    let mut schema =
        json!({ "type": "object", "properties": properties, "additionalProperties": false });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn no_args() -> Value {
    object(&[], &[])
}

/// A list tool's arguments (`shape::ListArgs`) plus its own.
fn list_args(own: &[(&str, Value)]) -> Value {
    let mut properties = own.to_vec();
    properties.extend([
        ("limit", limit()),
        ("cursor", cursor()),
        (
            "fields",
            json!({ "type": "array", "items": { "type": "string" },
                    "description": "Columns to return; default a compact set." }),
        ),
        ("filter", string("Case-insensitive substring.")),
        ("sort", string("A field; prefix - for descending.")),
    ]);
    object(&properties, &[])
}

fn target() -> Value {
    string("online (default), D: for Windows on another drive, or image:<mount dir>.")
}

fn limit() -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": 500, "description": "Default 50." })
}

fn cursor() -> Value {
    string("next_cursor of the previous page.")
}

fn string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn boolean(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_are_unique_and_snake_case() {
        let all = tools();
        let names: HashSet<_> = all.iter().map(|t| t.def.name).collect();
        assert_eq!(names.len(), all.len());
        for name in names {
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{name}"
            );
        }
    }

    #[test]
    fn only_plan_apply_is_destructive_and_it_needs_an_apply_tier() {
        for t in tools() {
            let a = t.def.annotations;
            if t.def.name == "plan_apply" {
                assert!(a.destructive && !a.read_only);
                assert!(t.tier >= Tier::ApplyReversible);
            } else {
                assert!(a.read_only && !a.destructive, "{}", t.def.name);
                assert!(t.tier <= Tier::Plan, "{}", t.def.name);
            }
        }
    }

    #[test]
    fn plans_need_the_plan_tier() {
        for t in tools() {
            if t.def.name.ends_with("_plan") {
                assert_eq!(t.tier, Tier::Plan, "{}", t.def.name);
            }
        }
    }

    #[test]
    fn schemas_are_closed_objects_whose_required_fields_exist() {
        for t in tools() {
            let s = &t.def.input_schema;
            assert_eq!(s["type"], "object", "{}", t.def.name);
            assert_eq!(s["additionalProperties"], false, "{}", t.def.name);
            for r in s["required"].as_array().into_iter().flatten() {
                let r = r.as_str().unwrap();
                assert!(s["properties"].get(r).is_some(), "{}: {r}", t.def.name);
            }
        }
    }

    #[test]
    fn toolsets_and_tiers_round_trip_their_names() {
        for t in Toolset::ALL {
            assert_eq!(Toolset::parse(t.name()), Some(t));
        }
        for t in Tier::ALL {
            assert_eq!(Tier::parse(t.name()), Some(t));
        }
        assert!(Tier::Observe < Tier::Plan && Tier::Plan < Tier::ApplyReversible);
    }

    #[test]
    fn enabled_keeps_catalog_order() {
        let core = enabled(&[Toolset::Core]);
        assert!(core.iter().all(|t| t.toolset == Toolset::Core));
        let all: Vec<_> = tools().into_iter().map(|t| t.def.name).collect();
        let both: Vec<_> = enabled(&Toolset::DEFAULT)
            .into_iter()
            .map(|t| t.def.name)
            .collect();
        assert_eq!(both, all);
    }
}
