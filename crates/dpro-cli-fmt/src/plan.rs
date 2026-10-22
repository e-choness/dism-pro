//! Plan preview text: the CLI equivalent of the Preview drawer (TD-0001 §4, TD-0002 §4).

use dpro_core::{
    plan::{ChangeKind, PlanDoc, Reversibility},
    units::Bytes,
};

use crate::{
    messages::Messages,
    table::{Table, render},
    units::format_bytes,
};

/// The main object a change acts on, for the "Object" column.
#[must_use]
pub fn subject(kind: &ChangeKind<String>) -> String {
    use ChangeKind as K;
    match kind {
        K::FileQuarantine { path, .. }
        | K::FileDelete { path, .. }
        | K::PendingQuarantine { path, .. }
        | K::TaskState { path, .. }
        | K::TaskDelete { path, .. }
        | K::PackageAdd { path }
        | K::FileRestore { path, .. } => path.clone(),
        K::RegSet { key, value, .. } => format!(r"{key}\{value}"),
        K::RegDelete { key, value } => value.as_ref().map_or_else(|| key.clone(), |v| format!(r"{key}\{v}")),
        K::ServiceStart { name, .. } | K::ServiceDelete { name, .. } | K::FeatureState { name, .. } => name.clone(),
        K::StartupEntry { id, .. } => id.clone(),
        K::FirewallRuleDelete { rule } => rule.clone(),
        K::AppxRemove { full_name, .. } => full_name.clone(),
        K::PackageRemove { identity } => identity.clone(),
        K::DriverAdd { inf } => inf.clone(),
        K::DriverRemove { published } => published.clone(),
        K::MsiRegistrationDelete { product_code, .. } => product_code.clone(),
        K::Native { api, .. } => api.clone(),
        K::ExternalProgram { command, .. } => command.clone(),
    }
}

/// Summary line plus a table of every change.
#[must_use]
pub fn render_plan(plan: &PlanDoc, m: &Messages, width: usize) -> String {
    if plan.changes.is_empty() {
        return format!("{}\n", m.text("cli-nothing-to-do"));
    }
    let s = &plan.summary;
    let mut out = m.get(
        "cli-plan-summary",
        &[
            ("count", &s.changes),
            ("bytes", &format_bytes(s.unique_bytes, m)),
            ("irreversible", &s.irreversible),
        ],
    );
    out.push_str("\n\n");

    let mut table = Table::new(&[
        (m.text("col-action"), false),
        (m.text("col-object"), false),
        (m.text("col-size"), true),
        (m.text("col-undo"), false),
    ]);
    for c in &plan.changes {
        let (bytes, _) = c.kind.freed();
        let size = if bytes == Bytes::ZERO { String::new() } else { format_bytes(bytes, m) };
        let mut badge = match &c.reversibility {
            Reversibility::Irreversible { .. } => m.text("badge-permanent"),
            Reversibility::ReversibleUntil { until } => m.get("badge-undoable-until", &[("until", &until.0)]),
            Reversibility::Reversible | Reversibility::ReversibleVia { .. } => m.text("badge-undoable"),
        };
        if c.reboot_required {
            badge.push_str(" · ");
            badge.push_str(&m.text("badge-reboot"));
        }
        table.push(vec![m.text(&format!("change-{}", c.kind.name())), subject(&c.kind), size, badge]);
    }
    out.push_str(&render(&table, width));
    out
}

#[cfg(test)]
mod tests {
    use dpro_core::{
        id::TargetId,
        plan::{Change, OpDescriptor, OsBuild, PLAN_SCHEMA, PlanSummary},
    };

    use super::*;

    fn doc(changes: Vec<Change<String>>, summary: PlanSummary) -> PlanDoc {
        PlanDoc {
            schema: PLAN_SCHEMA.into(),
            target: TargetId([0; 16]),
            os_build: OsBuild { major: 10, minor: 0, build: 26100, revision: 0 },
            op: OpDescriptor { id: "x".into(), title_key: "x".into(), category: "x".into(), cancellable: true },
            changes,
            summary,
        }
    }

    #[test]
    fn empty_plan_says_nothing_to_do() {
        assert_eq!(render_plan(&doc(vec![], PlanSummary::default()), &Messages::en_us(), 80), "Nothing to do.\n");
    }

    #[test]
    fn renders_summary_and_rows() {
        let change = Change {
            kind: ChangeKind::FeatureState { name: "SMB1Protocol".into(), enable: false, source: None },
            reversibility: Reversibility::Irreversible { reason: "rev-x".into() },
            reboot_required: true,
        };
        let summary = PlanSummary { changes: 1, irreversible: 1, reboot_required: true, ..PlanSummary::default() };
        let out = render_plan(&doc(vec![change], summary), &Messages::en_us(), 100);
        assert!(out.starts_with("1 changes · 0 B · 1 permanent\n"), "{out}");
        assert!(out.contains("Change feature"));
        assert!(out.contains("SMB1Protocol"));
        assert!(out.contains("Permanent · Reboot"));
    }
}
