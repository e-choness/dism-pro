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
