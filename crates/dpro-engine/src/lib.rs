//! Domain services: job runner, plan/apply, journal, target actors (TD-0001).
//!
//! Layer: domain. Never mutates the OS outside Plan → journal → apply.
//! Allowed workspace dependencies: `dpro-core`, `dpro-rules`, `dpro-win`.

#![forbid(unsafe_code)]

mod crc32c;
pub mod job;
pub mod journal;
