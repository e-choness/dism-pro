//! MCP protocol, tool registry, result shaping and policy (TD-0013).
//!
//! Layer: presentation.
//! Allowed workspace dependencies: `dpro-core`, `dpro-rules`, `dpro-app`, `dpro-broker`.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod policy;
pub mod protocol;
pub mod shape;
pub mod tool;
