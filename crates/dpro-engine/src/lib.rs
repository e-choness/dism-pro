//! Domain services: job runner, plan/apply, journal, target actors (TD-0001).
//!
//! Layer: domain. Never mutates the OS outside Plan → journal → apply.
//! Allowed workspace dependencies: `dpro-core`, `dpro-rules`, `dpro-win`.

#![forbid(unsafe_code)]

pub mod actor;
mod crc32c;
pub mod discover;
#[cfg(windows)]
pub mod dism_backend;
pub mod health;
pub mod identity;
pub mod job;
pub mod journal;
pub mod ops;
pub mod paths;
pub mod registry;
