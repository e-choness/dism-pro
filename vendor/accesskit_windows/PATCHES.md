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
       - A cell is at each column it spans: its `column_index` and the `column_span - 1` after
         it (TD-0009 slice 6b). A grouped table's header row is one row header spanning every
         column, so `GetItem` finds it at any of them.
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

7. **Notifications** (TD-0009 A4.5, slice 7b). Narrator speaks a job's progress steps, "Copied."
   and counts without a focus move through UI Automation notifications, which upstream doesn't
   raise.
   - **`src/adapter.rs`:**
     - `Adapter::notify(kind, processing, text, activity) -> Option<QueuedEvents>` queues a
       notification on the tree's root, with the text and activity as BSTRs. It returns `None`
       while no client has asked for the tree (the adapter isn't active), and where the function
       is missing. The design's `node` argument is left out: DISM Pro raises every notice on the
       main window's root (A4.5), and `accesskit_consumer` exposes no way to find a node by its
       local id. `notify_if` takes whether the function exists, so a test can take the path where
       it doesn't.
     - `UiaRaiseNotificationEvent` (Windows 10 1709) is looked up once with `GetProcAddress` on
       the loaded `uiautomationcore.dll`, cached in a `OnceLock`, and transmuted to its
       documented signature under `// SAFETY:`. It's never a static import, so the exe still
       starts on a WinPE older than 1709, where notices are dropped.
     - `QueuedEvents::raise` raises a notification with it, freeing the BSTRs after the call. A
       refusal is counted as item 11's are. `raise_with`'s signature doesn't change.
   - **`src/util.rs`:** `QueuedEvent::Notification`.
   - **New file:** `src/tests/notify.rs`: nothing is queued before a client asks; once one has,
     a notification is queued exactly when the function exists, and raised without a panic; with
     the function forced missing, nothing is queued. The tests run on the window's own thread
     with no client, so Wine runs them too (T2), and print which path it takes. Measured in T2:
     the image's Wine exports the function and accepts the raise. They activate the adapter
     through `handle_wm_getobject` without handing its answer on, because Wine faults in
     `UiaReturnRawElementProvider` from the harness's window (why T2 skips `tests::simple`).
     `cargo xtask pe-compat --exe` checks that G1's shell probe doesn't import the function.
   - **`src/tests/mod.rs`:** the `mod notify;` line.

8. **HeadingLevel** (TD-0009 A4.5, slice 6a). Narrator's scan mode jumps by heading (H, and 1–9
   by level) through UI Automation's HeadingLevel, which upstream doesn't report: it sends a
   heading's level as Level.
   - **`src/node.rs`:** a `Heading` with AccessKit level *n*, which is zero-based (aria-level less
     one), reports HeadingLevel 80051 + *n* (`HeadingLevel1` for level 0), a level past 8 as
     level 9, and no Level. A heading without a level, and every other role, reports none, which
     clients read as `HeadingLevel_None`. Other roles keep their Level.
9. **Landmarks** (TD-0009 A4.5, slice 6a). Narrator's D and its landmark list read LandmarkType,
   which upstream doesn't report.
   - **`src/node.rs`:**
     - `Main`, `Navigation` and `Search` report LandmarkType 80002, 80003 and 80004.
     - A `Region` with a name and a role description is a custom landmark: LandmarkType Custom
       (80000), with its role description as LocalizedLandmarkType, which UI Automation requires
       with Custom. Its LocalizedControlType is then UI Automation's default ("group"), instead
       of the role description that upstream gives every role.
     - A region without a name or a role description is no landmark, as upstream.
     - `Form`, `Banner`, `Complementary` and `ContentInfo` stay unmapped: nothing uses them.
10. **AcceleratorKey** from `keyboard_shortcut` (TD-0009 A4.5, slice 6a): a command button carries
    its accelerator as the command registry displays it ("Ctrl+Shift+R"). AccessKey stays
    unmapped: access keys are a non-goal (A4.2).
    - **`src/node.rs`:** the property, in the `properties!` table with 8's and 9's, so a change to
      any of them raises its property-changed event as every mapped property's does.
    - **New file:** `src/tests/landmarks.rs`, the tests of 8–10, called on the providers in
      process.
11. **Raising never panics** (TD-0009 A4.5, slice 6a). Upstream unwraps the results of
    `UiaRaiseAutomationEvent` and `UiaRaiseAutomationPropertyChangedEvent`. Under the release
    profile's `panic = "abort"`, one refused raise would end the process, the elevated one
    included.
    - **`src/adapter.rs`:** `QueuedEvents::raise(self) -> usize` raises every event and returns
      how many UI Automation refused (an `HRESULT` error); the events after a refused one are
      still raised. Upstream returns nothing. It calls `raise_with`, which takes the two raisers
      so that a test can refuse a raise. `QueuedEvents::of` (tests only) builds a queue.
    - **New file:** `src/tests/menu_events.rs`: a refused raise is counted and the events after
      it are raised; an empty queue counts nothing; and the real raise of event id 0, an unknown
      event, doesn't panic. The T2 image's Wine accepts that raise (measured: 0 refused), so the
      count's test refuses through `raise_with`. Whether Windows refuses it is to verify in T3.
      Items 12 and 13's tests join the file.
    - **`src/tests/mod.rs`:** the `mod landmarks;` and `mod menu_events;` lines.

12. **MenuOpened and MenuClosed** (TD-0009 A4.5, slice 7c). UI Automation's Menu control type
    must raise them, and upstream raises neither (its `TODO: handle other events (#20)`).
    - **`src/adapter.rs`:**
      - `open_close_events` gives a node's pair: MenuOpened and MenuClosed for `Role::Menu`, the
        only role that maps to the Menu control type. It gives none for a node the filter leaves
        out, or for any other role.
      - **Opened:** in `node_added`, on the node's platform node. Also in `node_updated`, for a
        menu a client starts seeing (shown again, or its role changed), as upstream does for
        dialogs.
      - **Closed:** in `node_removed`, on the platform node a client may hold, taken before
        `remove_platform_node`. Also in `node_updated`, for a menu a client stops seeing
        (hidden), on its platform node, so every opened has its closed.
      - `queue_closed` puts each closed event ahead of the update's other events. The tree
        reports additions first, so without it a popup replaced in one update would open before
        the old one closes.
      - **The placeholder's whole tree** (`update_if_active`, a client asked while the app was
        busy): every menu in it a client sees is opened, in tree order, before the focus event.
        A tree that activates the adapter from inactive raises nothing, as upstream raises no
        focus there: it's the answer to the client's own first ask (`WM_GETOBJECT`), which reads
        the menu anyway.
    - **Limit:** like upstream's dialogs, the filter is asked of the node alone. A menu under a
      hidden node would raise the pair as it's added and removed. DISM Pro's menus are their
      popup window's only child, never under a hidden node.
    - **The toolkit's side:** dpro-ui keeps a popup's window for its next open (TD-0009 A1.5),
      but builds a hidden popup's tree empty, so each open is new nodes: a removal and an
      addition here, and so the pair at every open.
    - **`src/tests/menu_events.rs`:** the tests of 12 and 13 join 11's.
      - A menu or a tooltip added opens, before the focus moves into it. Renamed, it raises
        neither. Removed, it closes on the same platform node, which then answers
        `UIA_E_ELEMENTNOTAVAILABLE`.
      - A popup rebuilt for each open raises the pair each time. Replaced in one update, it closes
        before the new one opens. Hidden and shown again, it closes, then opens.
      - A placeholder's whole tree opens the menus and tooltips it holds, but not hidden ones.
      - A refused MenuOpened is counted, and the focus event after it is still raised.
      - The real raise of the pair doesn't panic. Measured in T2: the image's Wine refuses none.
      - The tests activate the adapter through `handle_wm_getobject` without handing its answer
        on, as 7's do, and set the hidden window's focus state, since the adapter raises focus
        events only for a focused window. Whether a client hears the pair is G1's to check
        ("Accessibility depth: menus").
13. **ToolTipOpened and ToolTipClosed** (TD-0009 A4.5, slice 7c), the same as 12 for
    `Role::Tooltip`, the only role that maps to the ToolTip control type. The code and tests are
    12's.
    - dpro-ui's tooltip window shows a new node for a tip of another node. Changed text for the
      same node is a name change, with no pair.
