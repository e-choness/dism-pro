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
     - `src/tests/table.rs`: tests that call the providers in process, so Wine runs them too (T2),
       with the helpers in `src/tests/providers.rs` (5).
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
5. **A menu's check items are menu items** (TD-0009 5b2's column menu). A `MenuItemCheckBox` is
   UI Automation's MenuItem with the Toggle pattern, as Windows' own menus and WinUI's toggle menu
   items are, so Narrator reads "menu item, checked" and counts it among the menu's items.
   Upstream maps it to CheckBox, as Chromium does.
   - **`src/node.rs`:** the control type. `MenuItemRadio` is unused and stays upstream's.
   - **New files:** `src/tests/menu.rs`, its tests; `src/tests/providers.rs`, the helpers it
     shares with `src/tests/table.rs` (4).
   - **`src/tests/mod.rs`:** the `mod menu;` and `mod providers;` lines.
6. **Rows realized on request** (TD-0009 A2.5, slice 5c3). A virtualized table builds only the
   rows near its view, so the tree holds those alone, but a client can ask for any row: Narrator's
   Ctrl+Alt+End through Grid's `GetItem`, or a search through ItemContainer. The app can build a
   row when asked; the adapter asks it.
   - **New files:**
     - `src/realize.rs`: `Realize` (a row by index, or the first after one whose name is a
       given string), `Realized` (the row's index, not found, or unavailable), and the app's
       `Realizer`. `RealizingHandler` pairs the app's action handler with its realizer; the
       realizer is called without the handler's lock.
     - `src/tests/realize.rs`: in-process tests, the app's half played by a realizer that
       builds the row and pushes the update before it answers.
   - **`src/adapter.rs`:** `Adapter::with_realizer`: `Adapter::new`, given the app's realizer too.
   - **`src/context.rs`:** `ActionHandlerNoMut::realize`, unavailable by default, and
     `Context::realize`.
   - **`src/table.rs`:** a row by index, the rows built (with their selection) and the rows after
     one, through a filter that keeps rows a clipping parent hides, as the app built them; and a
     row's or cell's place in its table.
   - **`src/node.rs`:**
     - **Realizing.** A provider looks under the tree's read lock; when the row isn't there it
       drops the lock, asks `Context::realize`, and looks again under a new one. The app answers
       on its own thread, having pushed its update, so the second look finds the row.
     - **Grid's `GetItem`** on a row in range that isn't built asks for it: found, the cell;
       unavailable or still missing, `UIA_E_ELEMENTNOTAVAILABLE`, as before.
     - **ItemContainer on a table.** `FindItemByProperty` returns a row, after `pStartAfter`
       (a row or cell of this table; any other element is `E_INVALIDARG`) or from the first:
       - property 0: the next row, realized if it isn't built;
       - `Name` (a string): the app searches its view for the row with that name and realizes it.
         With no realizer, or one that can't answer, the built rows are searched;
       - `SelectionItemIsSelected` (a bool): the next selected row among those built (only the
         app's selection is known, and it's built: dpro-ui builds the current row), or the next
         unselected one, realized;
       - any other property, or a value of the wrong type, is `E_INVALIDARG`. No row is NULL with
         `S_OK`, as UI Automation's documentation asks.
   - **`src/lib.rs`, `src/tests/mod.rs`:** the `mod realize;` lines and the re-exports.

**Planned, with their tests written first** (TD-0009 A4.5, slice 6a; A4 numbers them 7–13, and
item 7, notifications, comes with slice 7b):
8. **HeadingLevel**, 9. **landmarks**, 10. **AcceleratorKey**: `src/tests/landmarks.rs`,
   ignored until the patches land.
11. **Raising never panics.** `QueuedEvents::raise` already returns `usize`, the failed raises'
   count; until the patch it's always 0 and a failed raise still panics.
   `QueuedEvents::of` (tests only) builds a queue. `src/tests/menu_events.rs` holds the count's
   test, ignored until the patch, and items 12–13's tests later.
   - **`src/tests/mod.rs`:** the `mod landmarks;` and `mod menu_events;` lines.
