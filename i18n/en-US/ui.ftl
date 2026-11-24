# dismpro GUI messages (TD-0009 §4). Source locale: en-US (ADR-0005).

## Settings › Appearance: why Glass (Auto) still shows the solid look (TD-0009 §4.5).
## One line per `SolidReason` (dpro-ui `materials`).

ui-solid-reason-winpe = Glass is off: Windows PE can't show it.
ui-solid-reason-windows-too-old = Glass is off: it needs Windows 11 version 22H2 or later.
ui-solid-reason-no-hardware-gpu = Glass is off: this PC has no graphics hardware to draw it with.
ui-solid-reason-device-lost = Glass is off: the graphics driver stopped responding. Restart DISM Pro to try again.
ui-solid-reason-high-contrast = Glass is off while a contrast theme is on.
ui-solid-reason-user-choice = Glass is off.
ui-solid-reason-transparency-off = Glass is off: transparency effects are off in Windows.
ui-solid-reason-remote-session = Glass is off during a remote desktop session.
ui-solid-reason-battery-saver = Glass is off while battery saver is on.

## Badges (TD-0009 A3.4, A3.5 (1)): the GUI's own. `badge-undoable` and `badge-permanent` are
## the catalog's shared words. `dpro_app::badges` picks the ID and its arguments.

ui-badge-undoable-days = { $days ->
        [one] Undoable 1 day
       *[other] Undoable { $days } days
    }
ui-badge-undoable-until-desc = Can be undone until { $date }.
ui-badge-undoable-via-desc = Undo runs: { $inverse }.
ui-badge-permanent-desc = Can't be undone: { $reason }
ui-badge-expired = Undo expired
ui-badge-restart = Restart required
ui-badge-no-restart = No restart needed
ui-badge-partial = Partially applied
ui-badge-running = Running

## Category scan cards (A3.5 (5)): the size slot's words, and Details when a scan failed.

ui-scan-scanning = Scanning…
ui-scan-nothing = Nothing to clean
ui-scan-failed = Couldn't scan this category.
ui-scan-details = Details
ui-size-unknown = —

## Info bars (A3.5 (14)).

ui-infobar-dismiss = Dismiss

## What joins the parts of a description the toolkit assembles from its props: a scan card's
## "{description}. {size}. {badge}", a clickable summary card's "{value}. {detail}".

ui-description-separator = .{" "}

## The inline error panel (TD-0009 §4.2, A3.5 (9)): the cause, the next step, and on Details the
## code, the call and the log. A finding's diagnosis (TD-0006) takes the title's and the next
## step's place when there is one.

# $what (String) - what was being done, from the page ("list the drivers")
ui-error-title = Couldn't { $what }.
# $failed (Number) - items that couldn't be read
# $total (Number) - items asked for
ui-error-partial = { $failed } of { $total } couldn't be read.
# $code (String) - the HRESULT, as 0x and eight hex digits
ui-error-code = Code { $code }
ui-error-next = Try again. If it keeps failing, copy the details and attach them to a bug report.
ui-error-details = Details
# $log (String) - the log's path
ui-error-log = From { $log }
# $api (String) - the system call that failed
ui-error-api = Call: { $api }
ui-error-copy = Copy details
ui-error-retry = Try again
ui-error-open-log = Open log
ui-copy-failed = Couldn't copy: another app is using the clipboard.

## The batch selection bar (A3.5 (11)), while a table has checked rows.

# $count (Number) - checked rows
ui-batch-selected = { $count } selected
# $hidden (Number) - checked rows the filter hides
ui-batch-hidden = { $hidden ->
    [one] 1 is hidden by the filter
   *[other] { $hidden } are hidden by the filter
}
ui-batch-clear = Clear selection
ui-batch-region = Selection actions

## The Preview drawer's frame (A3.5 (6)), slice 6d.

ui-drawer-close = Close
ui-drawer-cancel = Cancel
ui-code-copy = Copy command

## The command palette (A3.5 (3)).

ui-palette-name = Command palette
ui-palette-field = Search commands
ui-palette-placeholder = Type a command, page or setting
ui-palette-results = Commands
# $query (String) - what was typed
ui-palette-none = No commands match “{ $query }”.
ui-palette-hint-move = ↑↓ to move
ui-palette-hint-open = Enter to open
ui-palette-hint-close = Esc to close

## A job's toast (A3.5 (7)).

ui-toast-region = Job status
# $job (String) - the job's name
# $percent (Number) - how far it has come
ui-toast-progress = { $job }: { $percent } %
# $job (String) - the job's name
ui-toast-done = { $job } finished.
# $job (String) - the job's name
ui-toast-done-restart = { $job } finished. Restart Windows to complete it.
# $job (String) - the job's name
# $applied (Number) - changes applied
# $total (Number) - changes planned
ui-toast-failed = { $job } failed. { $applied } of { $total } changes were applied.
# $job (String) - the job's name
# $applied (Number) - changes applied
# $total (Number) - changes planned
ui-toast-cancelled = { $job } was cancelled after { $applied } of { $total } changes.
# $job (String) - the job's name
# $reason (String) - why it stopped
ui-toast-aborted = { $job } stopped: { $reason }
ui-toast-view = View
ui-toast-undo = Undo…
ui-toast-dismiss = Dismiss
