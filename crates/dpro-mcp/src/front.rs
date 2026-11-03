//! The unelevated front's [`Tools`] (TD-0013 §5.1). It lists the catalog the policy enables and
//! gates every call by the policy before anything runs, so a refused call never brings up a UAC
//! prompt. Pure tools run in this process; the rest go to the elevated broker, which reads the
//! policy again and is the check that counts.
//!
//! Tool errors start with the state an agent can act on (TD-0013 §4.4, §9): `policy_forbids`,
//! `policy_invalid`, `elevation_declined`, `broker_unavailable`.

use dpro_broker::wire::Outcome;
use serde_json::Value;

use crate::{
    catalog::{self, Runs, Tool, Toolset},
    policy::Policy,
    tool::{CallError, Caller, ToolDef, Tools},
};

/// What the front can't do on its own; `dismpro-cli` implements it.
pub trait Backend {
    /// Runs a tool in the broker, starting the broker on first use (one UAC prompt).
    ///
    /// # Errors
    /// [`Unavailable`] when no trusted broker answers.
    fn broker(&self, tool: &str, args: Value, caller: &Caller) -> Result<Outcome, Unavailable>;

    /// Runs one of the catalog's front tools.
    ///
    /// # Errors
    /// [`CallError`].
    fn local(&self, tool: &str, args: Value) -> Result<Value, CallError>;
}

/// Why no broker answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// The user dismissed the UAC prompt.
    ElevationDeclined,
    /// It couldn't be started, reached or trusted, or it went away.
    Broker(String),
}

/// The front: the policy, the tools it enables, and the way to run them.
#[derive(Debug)]
pub struct Front<B> {
    /// The policy, or why it couldn't be read (then every call fails with that reason).
    policy: Result<Policy, String>,
    tools: Vec<Tool>,
    defs: Vec<ToolDef>,
    backend: B,
}

impl<B: Backend> Front<B> {
    /// `requested`: the toolsets the client's configuration asks for (`--toolsets`), which can
    /// only narrow what the policy enables; `None` takes all of them.
    pub fn new(policy: Result<Policy, String>, requested: Option<&[Toolset]>, backend: B) -> Self {
        let allowed = match &policy {
            Ok(p) => p.toolsets.clone(),
            Err(_) => Toolset::DEFAULT.to_vec(),
        };
        let toolsets: Vec<Toolset> = allowed
            .into_iter()
            .filter(|t| requested.is_none_or(|r| r.contains(t)))
            .collect();
        let tools = catalog::enabled(&toolsets);
        let defs = tools.iter().map(|t| t.def.clone()).collect();
        Self {
            policy,
            tools,
            defs,
            backend,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    fn front_tools(&self) -> String {
        let names: Vec<&str> = self
            .tools
            .iter()
            .filter(|t| t.runs == Runs::Front)
            .map(|t| t.def.name)
            .collect();
        names.join(", ")
    }
}

impl<B: Backend> Tools for Front<B> {
    fn list(&self) -> &[ToolDef] {
        &self.defs
    }

    fn call(&self, name: &str, args: Value, caller: &Caller) -> Result<Value, CallError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.def.name == name)
            .ok_or_else(|| CallError::UnknownTool(name.to_owned()))?;
        let policy = self
            .policy
            .as_ref()
            .map_err(|e| CallError::Failed(format!("policy_invalid: {e}")))?;
        policy
            .check_tool(tool)
            .map_err(|e| CallError::Failed(format!("policy_forbids: {e}")))?;
        if tool.runs == Runs::Front {
            return self.backend.local(name, args);
        }
        match self.backend.broker(name, args, caller) {
            Ok(Outcome::Ok(v)) => Ok(v),
            Ok(Outcome::Invalid(m)) => Err(CallError::Invalid(m)),
            Ok(Outcome::Failed(m)) => Err(CallError::Failed(m)),
            Ok(Outcome::UnknownTool(n)) => Err(CallError::UnknownTool(n)),
            Err(Unavailable::ElevationDeclined) => Err(CallError::Failed(format!(
                "elevation_declined: {name} needs DISM Pro's administrator helper, and the \
                 administrator prompt was dismissed. Ask the user to accept it, then retry. \
                 Tools that work without it: {}.",
                self.front_tools()
            ))),
            Err(Unavailable::Broker(why)) => {
                Err(CallError::Failed(format!("broker_unavailable: {why}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;
    use crate::catalog::Tier;

    #[derive(Default)]
    struct Fake {
        broker_calls: RefCell<Vec<String>>,
        unavailable: Option<Unavailable>,
    }

    impl Backend for Fake {
        fn broker(&self, tool: &str, args: Value, _: &Caller) -> Result<Outcome, Unavailable> {
            self.broker_calls.borrow_mut().push(tool.to_owned());
            if let Some(u) = &self.unavailable {
                return Err(u.clone());
            }
            Ok(match tool {
                "updates_list" => Outcome::Invalid("bad cursor".to_owned()),
                "health_check" => Outcome::Failed("DISM failed".to_owned()),
                _ => Outcome::Ok(json!({ "tool": tool, "args": args })),
            })
        }

        fn local(&self, tool: &str, args: Value) -> Result<Value, CallError> {
            Ok(json!({ "local": tool, "args": args }))
        }
    }

    fn front(policy: Result<Policy, String>, fake: Fake) -> Front<Fake> {
        Front::new(policy, None, fake)
    }

    #[test]
    fn routes_front_tools_locally_and_the_rest_to_the_broker() {
        let f = front(Ok(Policy::default()), Fake::default());
        let c = Caller::default();
        assert_eq!(
            f.call("explain_error", json!({ "code": "5" }), &c).unwrap(),
            json!({ "local": "explain_error", "args": { "code": "5" } })
        );
        assert_eq!(
            f.call("targets_list", json!({}), &c).unwrap()["tool"],
            "targets_list"
        );
        assert_eq!(*f.backend().broker_calls.borrow(), ["targets_list"]);
    }

    #[test]
    fn refused_calls_never_reach_the_broker() {
        let observe = Policy {
            tier: Tier::Observe,
            ..Policy::default()
        };
        let f = front(Ok(observe), Fake::default());
        let c = Caller::default();
        for tool in ["undo_plan", "health_repair_plan", "plan_apply"] {
            let e = f.call(tool, json!({}), &c).unwrap_err();
            assert!(
                matches!(&e, CallError::Failed(m) if m.starts_with("policy_forbids: ")),
                "{tool}: {e}"
            );
        }
        let plan = front(Ok(Policy::default()), Fake::default());
        let e = plan.call("plan_apply", json!({}), &c).unwrap_err();
        assert!(e.to_string().contains("apply_reversible"), "{e}");
        assert!(f.backend().broker_calls.borrow().is_empty());
        assert!(plan.backend().broker_calls.borrow().is_empty());
    }

    #[test]
    fn an_invalid_policy_refuses_everything_with_its_reason() {
        let f = front(Err("line 3: unknown key".to_owned()), Fake::default());
        let e = f
            .call("explain_error", json!({}), &Caller::default())
            .unwrap_err();
        assert_eq!(
            e,
            CallError::Failed("policy_invalid: line 3: unknown key".to_owned())
        );
        // The list still shows the default catalog, so the agent can report the problem.
        assert_eq!(f.list().len(), catalog::enabled(&Toolset::DEFAULT).len());
    }

    #[test]
    fn broker_outcomes_map_to_call_errors() {
        let f = front(Ok(Policy::default()), Fake::default());
        let c = Caller::default();
        assert_eq!(
            f.call("updates_list", json!({}), &c),
            Err(CallError::Invalid("bad cursor".to_owned()))
        );
        assert_eq!(
            f.call("health_check", json!({}), &c),
            Err(CallError::Failed("DISM failed".to_owned()))
        );
        assert_eq!(
            f.call("no_such_tool", json!({}), &c),
            Err(CallError::UnknownTool("no_such_tool".to_owned()))
        );
    }

    #[test]
    fn a_declined_prompt_names_what_still_works() {
        let fake = Fake {
            unavailable: Some(Unavailable::ElevationDeclined),
            ..Fake::default()
        };
        let f = front(Ok(Policy::default()), fake);
        let e = f
            .call("targets_list", json!({}), &Caller::default())
            .unwrap_err();
        let m = e.to_string();
        assert!(m.starts_with("elevation_declined: "), "{m}");
        assert!(m.contains("explain_error"), "{m}");
        let gone = Fake {
            unavailable: Some(Unavailable::Broker("pipe closed".to_owned())),
            ..Fake::default()
        };
        let f = front(Ok(Policy::default()), gone);
        assert_eq!(
            f.call("targets_list", json!({}), &Caller::default()),
            Err(CallError::Failed(
                "broker_unavailable: pipe closed".to_owned()
            ))
        );
    }

    #[test]
    fn the_client_can_narrow_toolsets_but_not_widen_them() {
        let core_only = Policy {
            toolsets: vec![Toolset::Core],
            ..Policy::default()
        };
        let f = Front::new(Ok(core_only), Some(&Toolset::ALL), Fake::default());
        assert!(f.list().iter().all(|d| d.name != "health_check"));
        let f = Front::new(
            Ok(Policy::default()),
            Some(&[Toolset::Maintain]),
            Fake::default(),
        );
        assert!(f.list().iter().all(|d| d.name != "targets_list"));
        assert!(f.list().iter().any(|d| d.name == "health_check"));
        assert!(matches!(
            f.call("targets_list", json!({}), &Caller::default()),
            Err(CallError::UnknownTool(_))
        ));
    }
}
