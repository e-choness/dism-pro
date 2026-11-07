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
4. **The table and scroll patterns** (TD-0009 A2.5, slice 5c2). A DataGrid's Narrator navigation
   moves cell by cell (Ctrl+Alt+arrows) through Grid's `GetItem` and reads each cell's headers
   through TableItem.
   - **New files:**
     - `src/table.rs`: the table lookups, in the filtered tree the client sees.
       - A table is a `Table`, `Grid` or `TreeGrid` with `row_count` and `column_count`.
         AccessKit computes neither.
       - Its rows are `Row`s at any depth with a `row_index`.
       - Its cells are a row's `Cell`, `GridCell`, `RowHeader` and `ColumnHeader` parts with a
         `column_index`, at the row's index unless they carry their own.
       - A nested table's parts are its own.
     - `src/tests/table.rs`: tests that call the providers in process, so Wine runs them too (T2).
   - **`src/node.rs`:**
     - **Grid on a table.** `RowCount`, `ColumnCount`, and `GetItem`:
       - a row or column past the counts is `E_INVALIDARG`;
       - a row in range that isn't in the tree is `UIA_E_ELEMENTNOTAVAILABLE`.
     - **Table on a table.**
       - It's row-major.
       - `GetColumnHeaders` lists its column headers.
       - `GetRowHeaders` is empty: only the built rows' headers could be listed, and a cell's row
         header comes from its TableItem.
     - **GridItem and TableItem on a table's cells.**
       - Row, column, spans (1 if not given), and the containing grid.
       - The row's other row headers, and the column's other column headers.
     - **Control types.**
       - A column header that isn't a cell (one outside the rows) is a HeaderItem.
       - A row group whose first child is one is the Header.
       - Neither is a content element, as UI Automation's Header and HeaderItem types require.
       - Upstream maps both to DataItem and Group.
     - **ItemStatus** is the node's `state_description`: the app's words, in the user's language,
       such as a sorted header's "Sorted ascending".
     - **SelectionItem on a `Row`** with `selected`.
       - Its grid's `GetSelection` lists the selected rows and grid cells at any depth. Upstream
         lists only "item-like" children, which rows aren't.
       - `AddToSelection` on a row of a single-select grid with another row selected is
         `UIA_E_INVALIDOPERATION`.
       - So is `RemoveFromSelection` on a selected row, since the click that selects a row never
         deselects one.
     - **Scroll on any node with `scroll_x` or `scroll_y`.**
       - An axis scrolls when its maximum is past its minimum (0 if not given).
       - The view size is the node's bounds along the axis, as a share of the bounds and the range
         together.
       - `Scroll` sends `ScrollLeft`, `ScrollRight`, `ScrollUp` or `ScrollDown` with `ScrollUnit`:
         small is an item, large a page.
       - `SetScrollPercent` sends one `SetScrollOffset`. An axis given as
         `UIA_ScrollPatternNoScroll` keeps its offset.
       - A percentage outside 0–100 is `E_INVALIDARG`. An amount on an axis that doesn't scroll,
         or a missing action, is `UIA_E_INVALIDOPERATION`, and then nothing is sent.
   - **`src/lib.rs`, `src/tests/mod.rs`:** the two `mod table;` lines.
