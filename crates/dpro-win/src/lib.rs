//! Safe wrappers over Windows APIs: DISM session, WIM handle, registry, VSS, BCD, WUA, MSI,
//! ETW, SCM, tasks, firewall, security tokens. Every OS handle has a RAII owner.
//!
//! Layer: platform. One of the two crates allowed `unsafe`; every block needs a `// SAFETY:` comment.
//! Allowed workspace dependencies: `dpro-core`, `dpro-sys`.
