//! Safe wrappers over Windows APIs: DISM session, WIM handle, registry, VSS, BCD, WUA, MSI,
//! ETW, SCM, tasks, firewall, security tokens. Every OS handle has a RAII owner.
//!
//! Layer: platform. One of the two crates allowed `unsafe`; every block needs a `// SAFETY:` comment.
//! Allowed workspace dependencies: `dpro-core`, `dpro-sys`.
//!
//! Status (roadmap 2.3): written without a compiler. `dism`/`wim` sit on the S-03 bindings, which are
//! unverified until `cargo xtask ffi-check` passes; see `CAVEATS.md`.
//!
//! Everything here is `cfg(windows)`; on other targets the crate is empty.

#![cfg(windows)]

pub mod dism;
pub mod error;
pub mod privilege;
pub mod reg;
pub mod system;
pub mod wide;
pub mod wim;

pub use error::Error;
