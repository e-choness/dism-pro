//! CLI presentation: tables, plan rendering, byte formatting, messages, exit codes (TD-0002).
//!
//! Layer: presentation. Pure: no I/O, so everything here is unit-tested in C1.
//! Allowed workspace dependencies: `dpro-core`, `dpro-app`.

#![forbid(unsafe_code)]

pub mod exit;
pub mod messages;
pub mod plan;
pub mod table;
pub mod units;
