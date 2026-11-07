# accesskit_windows, as vendored by DISM Pro

**Upstream:** [AccessKit](https://github.com/AccessKit/accesskit) `adapters/windows`, release
0.35.2. That's commit `f3026222559c7f5f46d8f25cb7404de68eb149f0`, the crates.io package of
2026-10-25.

**Licence:** MIT OR Apache-2.0, as upstream (`LICENSE-MIT`, `LICENSE-APACHE`). `src/node.rs` is
also derived from Chromium (`LICENSE.chromium`). The three licence files come from the upstream
commit, since the crates.io package doesn't include them. DISM Pro's own changes are under the
same terms.

**Why:** UI Automation's DataGrid needs the Grid, Table, GridItem, TableItem, Scroll and
ItemContainer patterns. AccessKit has none of them yet (AccessKit#29), and the owner decided
DISM Pro builds what it needs here, not upstream (decisions-log #36; TD-0009 A2.5, ADR-0002's
amendment).

**Rules:**
- Every change is marked `dpro:` in the code and listed below.
- Everything else is upstream's code, untouched. That includes its formatting and lints: the
  crate isn't a workspace member, so the workspace's rustfmt and clippy settings don't apply.
- `accesskit` and `accesskit_consumer` stay on crates.io at the exact pins in the root
  `Cargo.toml`.
- **To take a new AccessKit release:**
  1. Copy its `adapters/windows` over this directory.
  2. Apply this list again.
  3. Bump the pins together, then run the upstream tests (T3, `accesskit-windows-tests.exe`) and
     G1.

## Changes

1. **`SubclassingAdapter` removed** (`src/subclass.rs`, `src/tests/subclassed.rs`, and their
   `mod` lines in `src/lib.rs` and `src/tests/mod.rs`). dpro-win owns its window procedure and
   uses `Adapter`, and the subclassing test needs winit.
2. **The tests' harness uses std** (`src/tests/mod.rs`).
   - `std::sync::Mutex` replaces parking_lot's. Its poison is ignored where it's locked, so a
     failed test doesn't fail the ones after it.
   - A small drop guard, `Defer`, replaces scopeguard's `guard`.
   - With 1, the manifest has no dev-dependencies. Cargo resolves none for a crate outside the
     workspace, and `cargo test -p accesskit_windows` builds the upstream tests without them.
3. **The manifest** is crates.io's normalized `Cargo.toml` for 0.35.2, without its
   dev-dependencies (1, 2). It's self-contained, as a crate outside the workspace needs.
