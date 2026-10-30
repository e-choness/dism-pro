//! Named-pipe IPC between the unelevated MCP front and the elevated agent broker (TD-0013, ADR-0006).
//!
//! Layer: platform. OS access goes through `dpro-win`.
//! Allowed workspace dependencies: `dpro-core`, `dpro-win`.

#![forbid(unsafe_code)]
