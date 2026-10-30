//! Raw FFI bindings for `dismapi.h` and `wimgapi.h` (spike S-03). Everything else comes
//! from the `windows`/`windows-sys` crates.
//!
//! Layer: FFI. One of the two crates allowed `unsafe`; every block needs a `// SAFETY:` comment.
//! Allowed workspace dependencies: none.
//!
//! Status: first cut, **unverified**. Every struct layout and function signature here must
//! pass `cargo xtask ffi-check` (C static asserts compiled against Microsoft's headers)
//! before `dpro-win` builds on it. See `layout` for the table the check is generated from.
//!
//! The struct mirrors are target-independent so their layout can be tested on Linux (T1);
//! the `extern` blocks exist only on Windows and link with `raw-dylib` (no import libraries).

pub mod dism;
pub mod layout;
pub mod types;
pub mod wim;
