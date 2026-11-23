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
