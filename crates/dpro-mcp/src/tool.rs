//! Tool definitions and the interface the protocol layer calls them through (TD-0013 §5.3).

use serde::Serialize;
use serde_json::Value;

/// The behaviour hints MCP clients show (TD-0013 §3): informative, never a safety boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // MCP's own shape: four independent hints.
pub struct Annotations {
    #[serde(rename = "readOnlyHint")]
    pub read_only: bool,
    #[serde(rename = "destructiveHint")]
    pub destructive: bool,
    #[serde(rename = "idempotentHint")]
    pub idempotent: bool,
    /// Talks to something outside the machine (Windows Update).
    #[serde(rename = "openWorldHint")]
    pub open_world: bool,
}

impl Annotations {
    /// A tool that only reads local state.
    pub const READ_ONLY: Self = Self {
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    };
}

/// One tool as `tools/list` describes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub annotations: Annotations,
}

/// Who made a call: per request in 2026-07-28, per process after a legacy `initialize`. The
/// client's name and version are its own claim (TD-0013 §8.3: recorded, labelled unverified).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caller {
    pub protocol_version: String,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

/// Why a call produced no result. `Invalid` and `Failed` reach the model as a tool error it can
/// act on (`isError: true`); `UnknownTool` is a protocol error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// The arguments don't fit the tool's input schema.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Failed(String),
}

/// The tools a server offers.
pub trait Tools {
    /// The catalog, in a stable order (TD-0013 §15: deterministic `tools/list`).
    fn list(&self) -> &[ToolDef];

    /// Runs one tool. `Ok` is its `structuredContent`.
    ///
    /// # Errors
    /// [`CallError`].
    fn call(&self, name: &str, args: Value, caller: &Caller) -> Result<Value, CallError>;
}
