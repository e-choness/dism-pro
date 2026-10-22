//! Core domain types: ids, errors, units, `Plan`/`Change`, progress, cancellation (TD-0001).
//!
//! Layer: pure. No OS dependencies; compiles and tests natively on Linux.
//! Allowed workspace dependencies: none.

#![forbid(unsafe_code)]

pub mod cancel;
pub mod error;
pub mod id;
pub mod intern;
pub mod plan;
pub mod progress;
pub mod protected;
pub mod target;
pub mod units;

pub use error::{Error, OsError};
