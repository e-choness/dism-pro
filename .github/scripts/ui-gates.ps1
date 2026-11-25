<#
TD-0009 §7, §10: G1's UI gates, on the windows-2025 runner's interactive desktop (1024x768 at
96 DPI; S-01 run 41589907500). Every gate is a ratio or a count against dpro-win's bare_window, the
raw-API baseline, measured in this job, so it holds on whatever machine the runner lands on
(doc 04 §3).

  U4  Startup: process start to the first painted frame (--ui-probe first-frame), bare_window and
      the probe target in interleaved rounds after a discarded warm-up round. The probe target
      runs twice: `shell` as it starts by default, and `glass` with --materials glass, which
      composes on WARP. Gate: each median <= 3x bare_window's (both presenters, TD-0009 §7).
      Exact gate: every report names the presenter it should. `shell` is GDI, because the runner's
      adapter is a software one (decision #30j); `glass` is DirectComposition.
  U6  Idle: from the first frame, 3 s to settle, then 5 s untouched (--ui-probe idle). Gate: no
      timer and no paint in those 5 s, for both. CPU time and messages are reported next to
      bare_window's.
  U7  Idle working set. Gate: <= 40 MiB (doc 04 §2) for both, at the 1000x700 window that fits the
      runner's screen. The budget's 1280x800 default adds 1.1 MiB of canvas (and, composed, two
      swap-chain buffers of the same size).
  U12 UI Automation, as a client sees it. The time from the window showing to the tree served
      (the "Review changes" button found through UIA) is advisory: <= 100 ms (S-01: 40-42 ms).
      The client is warmed on bare_window first, so its own start isn't counted.
      The checks are gates: names and roles (the page list, the table's rows, the check boxes),
      then Toggle, SetFocus on a row and Invoke through UIA, each read back through UIA. A table
      row's SelectionItem comes with slice 5c's adapter: accesskit_windows 0.35.2 gives none to
      a grid's rows ("TODO: tables").
  Live theme switch: see its section.
  Glass (ADR-0007, TD-0009 §5.2): the glass shell's window base, read off the screen, is DWM's
      backdrop and not the solid token. Switching transparency off shows the solid token, and on
      again brings the backdrop back, live, each within 2 s (advisory: 100 ms).
  Text input (TD-0009 §11 step 3): the shell's filter field through real input. Typed with
      SendInput (WM_CHAR, a surrogate pair included), Ctrl+A and Ctrl+C, Backspace, a two-line
      clipboard pasted with Ctrl+V as one line, Shift+Home and Ctrl+X; then UIA's SetValue, which
      filters the table, and the pointer's I-beam over the field. Each is read back through UIA or
      the clipboard within 2 s; the time from typing to UIA's value is advisory (U3: 50 ms).
  Controls (TD-0009 §11 step 4): the shell's tabs, rail, radio group, progress bar and Hold to
      apply, through UIA and real keys. SelectionItem.Select on a tab picks it and refilters the
      table; Left on the focused tab picks the other (selection follows focus); Down on the rail
      moves focus without opening a page, and Enter opens it; Select and Down on Settings' radio
      buttons pick, wrapping. The drawer's progress bar is a read-only RangeValue at 0; Invoke on
      Hold to apply asks for the typed confirmation and applies nothing; Enter held 0.5 s and let
      go applies nothing; held 1.8 s it applies (the bar reaches 100). Each is read back through
      UIA within 2 s.
  Text scaling (TD-0009 §12 item 7): Windows' text size, TextScaleFactor under
      HKCU\Software\Microsoft\Accessibility, written 225 then 100 with no broadcast: the shell's
      title, read through UIA, grows 2.25 times and comes back, each within 2 s (advisory: 100 ms).
      UISettings.TextScaleFactor (WinRT) is reported beside the registry value. The runner's own
      value is restored.
  Popups (TD-0009 A1.9): real keys and clicks, each result read back through UIA within 2 s.
      Shift+F10 on a focused table row shows a dismpro.popup window the shell owns (a tool
      window), and UIA's FocusedElement is its first MenuItem (A1.3's verify). Down moves the
      focus; Enter invokes the item, closes the menu and puts focus back on the row. A second
      Shift+F10 shows the same window again, and Esc closes it, invoking nothing. A right-click on
      a row picks it and opens its menu at the pointer; a click outside closes it and reaches
      nothing, and the next click there does; a right-click outside closes it and opens no other
      menu, and the next right-click there opens that row's. The pointer resting on Review
      changes shows its ToolTip, not before Windows' delay; the tip never activates and goes when
      the pointer leaves. Settings' Start page ComboBox: Expand opens its list focused on the picked option,
      Down and Enter pick the next, Select in the list picks another, Collapse closes it, each
      read back as the combo's value and ExpandCollapse state. Advisory (U3, 100 ms): the first
      and second menu opens, the right-click and the dropdown, from the input to UIA seeing it;
      and (U7, 2 MiB) the working set the open menu adds. Screenshots of the menu, the list and
      the tip go to -Out for A1.3's shadow and corners, checked by hand.
  Scrolling (TD-0009 A2, slices 5a and 5b1): the shell's table of 750 rows, a DataGrid whose
      rows are DataItems. A wheel notch through SendInput at its center moves the rows up, and a
      notch back puts them where they were; on a focused row, Ctrl+End makes the last row current,
      focused and in view, and Ctrl+Home the first again; UIA's SetFocus on a row out of view
      scrolls it into view. UIA sees every row the table builds, those in view and half a view
      either side: the rows' viewport is a generic container AccessKit leaves out, so a row's
      parent is the grid, which clips nothing (accesskit_consumer's common_filter drops only what a
      clipping parent hides, and then not the children next to those it shows). Each is read back as
      the rows' BoundingRectangle within 2 s; the times are advisory (U3: 50 ms), and Windows'
      wheel lines are reported. So is "Automatically hide scroll bars", for A2.3's verify:
      DynamicScrollbars, which the shell reads, beside UISettings.AutoHideScrollBars, with the
      value written both ways (no broadcast, before the shell starts) and put back.
  Columns (TD-0009 A2, slice 5b2): the shell's table header, through real mouse input and keys.
      The pointer on Publisher's divider (its HeaderItem's right edge) shows the left-right arrows
      (IDC_SIZEWE); pressed there and dragged 60 px right in 3 moves, let go, the HeaderItem is
      60 px wider, and Name's, left of it, holds still. A double-click there fits the column: its
      width changes, measured by DirectWrite, and holds. Shift+F10 on the focused Publisher header
      opens a Menu "Columns" in an owned popup, its hideable columns MenuItems with Toggle
      (Publisher On) and then its three commands; Toggle on Publisher closes the menu and takes
      its HeaderItem away, focus going to Version's; Shift+F10 there and Invoke on Reset columns
      brings it back at its first width. Each is read back through UIA within 2 s; the drag's time
      is advisory (U3: 50 ms).
  Value cells (TD-0009 A2.5, slice 5b3): the shell's sizes and days in the runner's formats. A
      row's Size cell reads with the user's decimal separator (GetLocaleInfoEx) and its Installed
      cell as GetDateFormatEx writes the day. The decimal separator and the short date are then
      changed with SetLocaleInfoW and WM_SETTINGCHANGE "intl" broadcast, as Settings does; the
      cells follow, and follow back when the old values are put back (always, in a finally). Each
      is read back through UIA within 2 s; the time is advisory (U3: 50 ms).
  Data table (TD-0009 A2.5, A2.9, slice 5c3): the shell with --rows 100000, 75,000 on its first
      tab, through UIA's patterns on the DataGrid. GridPattern counts every row while at most 100
      are built. GetItem on the last row's Publisher cell, which isn't built, returns it with its
      GridItem row and column, and the view has scrolled to it; its TableItem gives the Publisher
      HeaderItem and the row's name cell. ItemContainer's FindItemByProperty finds a row by Name
      halfway down (the shell searches its view and builds the row), in view; after it, none other
      has that name. ScrollPattern's SetScrollPercent(-1, 100) shows the last row, and a wheel
      notch (SendInput) changes the first row built. Toggle on a check cell checks the row, and the
      check column's header box goes Mixed (the probe's toolbar has no count); Toggle again clears
      both. The focused row's SelectionItem is selected. TablePattern's column headers are the
      header's HeaderItems; Invoke on Name's twice sorts by it and then reverses, so GetItem(0, 1)
      is the last row's name. Each is read back through UIA within 2 s; GetItem's and the search's
      round trips are advisory (U3: 50 ms).
  Composites (TD-0009 A3.5, slice 6c): the shell with --composites, slice 6c's page, through real
      keys and clicks. Ctrl+K shows the palette in an owned popup that takes the activation, its
      Edit "Search commands" focused; its List "Commands" has exactly one selected ListItem, and
      Down selects the next while focus stays in the field; typing filters the results; Esc closes
      it and the page is active again. Ctrl+J starts a job: its toast shows at the window's bottom
      end without taking the activation (and isn't WS_EX_NOACTIVATE), and a click on it leaves the
      activation; F6 gives it the keyboard (Dismiss focused), and F6 hands it back; Esc in it
      hides it, and the page is active again. Ctrl+E ends the job, which shows its toast again,
      and the toast hides once SPI_GETMESSAGEDURATION has passed (not before) with the pointer and
      the focus elsewhere. SPI_GETMESSAGEDURATION and SPI_GETCLIENTAREAANIMATION are reported for
      A3.5's verifies. Each check reads back within 2 s; the palette's opening is advisory (U15:
      100 ms from Ctrl+K to UIA seeing its field focused). Screenshots of the palette and the toast
      go to -Out.
  Composites: drawer (TD-0009 A3.5 (6), slice 6d): the drawer probe (examples/drawer.rs). Enter
      on "Review changes" opens the drawer, the Group "Preview" (a region landmark), focus on its
      first change; Esc closes it and focus goes back to "Review changes". Those are gates. A
      second run under --ui-probe idle counts the slide's frame timers: 12 with animations on
      (U16), none off; advisory, as the runner's timing is.
  Composites: picker dialog (TD-0009 A3.5 (12), slice 6e): the file dialog probe
      (crates/dpro-win/examples/file_dialog_t3.rs), choosing a folder, then a file. The system
      dialog, found through UIA by its title, is owned by the probe's window and disables it; the
      probe's tree answers through UIA while it's up; Invoke on Cancel closes it, the probe
      reports Ok(None) and the timer wakes its window heard through the modal loop, and afterwards
      the window is enabled and its tree answers again. Those are gates; Cancel to the answer is
      advisory (100 ms).
  Composites: picker U19 (TD-0009 A3.5 (12), A3.7 U19, slice 6e): the picker probe
      (examples/picker.rs) over a folder under %TEMP% holding one folder of 10,000 folders. As the
      app is, Enter on "Browse..." shows the system dialog "Choose a folder", owned by the probe's
      window, and Cancel in it leaves the page as it was, Browse... focused. With --in-app (no
      system dialog, as in WinPE), Enter on "Browse..." opens the Group "Choose a folder" (a region
      landmark), focus in its Edit "Path", whose Value is the folder; its DataGrid lists the one
      folder; Enter on that row lists all 10,000 (GridPattern's RowCount, within 10 s); Esc closes
      it and focus goes back to "Browse...". Those are gates. Enter to the first screen of rows and
      to all 10,000 are advisory (U19: the first screen <= 50 ms, U3), as the runner's NTFS is.
  Accessibility depth, notifications (TD-0009 A4.5, A4.7 U23, slice 7b): the shell with
      --job-ms 4000, whose drawer job runs 4 s from the drawer opening. A COM IUIAutomation5 client
      (managed UIA can't hear notifications) subscribes to the window's notifications, and Invoke on
      Review changes opens the drawer. Gates: exactly one progress notification over the job, at
      2 s or later, saying "Apply 20 changes: N0 percent", kind Other, processing MostRecent,
      activity DismPro.Progress.0. Its delivery after the step was due is advisory (U23: 100 ms).
  Accessibility depth, menus (TD-0009 A4.5, A4.4, slice 7c): the shell, and a COM IUIAutomation
      client on the desktop's subtree for MenuOpened, MenuClosed, ToolTipOpened and ToolTipClosed
      (the adapter's PATCHES.md 12 and 13). Each popup opens once first, since the first open
      activates its window's adapter with no opened event. Gates, each heard within 2 s: Esc
      closing a row's menu raises MenuClosed; Shift+F10 again raises MenuOpened on the Menu
      "Row"; Esc again raises MenuClosed, exactly one of each for that open. The pointer leaving
      Review changes raises ToolTipClosed, resting on it again ToolTipOpened with the tip's text,
      and leaving again ToolTipClosed. Shift+Tab back onto Review changes raises ToolTipOpened;
      that keyboard tip stays past ten double-click times with no ToolTipClosed (A4.4), and Esc
      raises it. The events' times are advisory (100 ms).

The probe target is dpro-ui's `shell` example until roadmap 3.2 gives dismpro.exe its shell.
Results go to annotations and the job summary; the raw reports and screenshots go to -Out, which
CI keeps as the ui-gates artifact.
#>
param(
    [string] $Bin = 'dist-t3',
    [string] $Out = 'ui-gates',
    [int] $Rounds = 9,
    [string] $Size = '1000x700'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

# A harness error becomes an annotation (where and what): the job log needs a token to read.
trap {
    $at = $_.InvocationInfo
    Write-Host "::error title=UI gates harness::$(Format-Annotation "line $($at.ScriptLineNumber): $("$($at.Line)".Trim())`n$($_.Exception.Message)")"
    exit 1
}

$Bin = (Resolve-Path $Bin).Path
$null = New-Item -ItemType Directory -Force $Out
$Out = (Resolve-Path $Out).Path
# The parameters are fixed from here on. PowerShell's names ignore case: Value cells' `$size = ...`
# once replaced -Size, and the Data table's shell then got `--size '37.0 MB'` and exited.
# (Set-Variable, not .Options: CI calls the script from another, where parameters are optimized
# locals whose Options can't be set.)
Set-Variable -Name Bin, Out, Rounds, Size -Option ReadOnly -Force
$exe = [ordered]@{ bare = Join-Path $Bin 'bare_window.exe'; shell = Join-Path $Bin 'shell.exe' }
foreach ($path in $exe.Values) {
    if (-not (Test-Path $path)) { throw "missing $path (docker compose run --rm t3-tools builds it)" }
}
# The probe targets: what runs, and the presenter its reports must name (composed=0 is GDI).
$targets = [ordered]@{
    bare  = @{ Path = $exe.bare; Args = @(); Composed = 0 }
    shell = @{ Path = $exe.shell; Args = @(); Composed = 0 }
    glass = @{ Path = $exe.shell; Args = @('--materials', 'glass'); Composed = 1 }
}
$names = @($targets.Keys)
$failed = 0

function Publish([string] $Title, [string[]] $Lines, [bool] $Ok) {
    $text = $Lines -join "`n"
    Write-Host "== $Title`n$text"
    "== $Title`n$text`n" | Out-File (Join-Path $Out 'summary.txt') -Append
    if ($env:GITHUB_STEP_SUMMARY) { "### $Title`n``````n$text`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append }
    $kind = if ($Ok) { 'notice' } else { 'error' }
    Write-Host "::$kind title=$Title::$(Format-Annotation $text)"
}

# Runs one probe to completion and returns its report's fields (name=value pairs) as numbers.
function Invoke-Probe([string] $Name, [string] $Kind, [int] $TimeoutSec = 60) {
    $report = Join-Path $Out "$Name-$Kind.txt"
    Remove-Item $report -ErrorAction SilentlyContinue
    $arguments = @($targets[$Name].Args) + @('--size', $Size, '--ui-probe', $Kind, '--probe-out', $report)
    $p = Start-Process -FilePath $targets[$Name].Path -ArgumentList $arguments -PassThru
    $null = $p.Handle # keeps the exit code readable after exit
    if (-not $p.WaitForExit($TimeoutSec * 1000)) {
        $p.Kill()
        throw "$Name --ui-probe $Kind reported nothing in $TimeoutSec s"
    }
    if ($p.ExitCode -ne 0 -or -not (Test-Path $report)) {
        throw "$Name --ui-probe $Kind exited with $($p.ExitCode) and no report"
    }
    $fields = @{}
    foreach ($pair in ((Get-Content $report -Raw).Trim() -split '\s+')) {
        $key, $value = $pair -split '=', 2
        $fields[$key] = [double] $value
    }
    # bare_window doesn't present through the toolkit's window, and reports no presenter.
    if (-not $fields.ContainsKey('composed')) { $fields.composed = 0 }
    if (-not $fields.ContainsKey('warp')) { $fields.warp = 0 }
    $fields
}

function Get-Median([double[]] $v) { $s = $v | Sort-Object; $s[[int][Math]::Floor(($s.Count - 1) / 2)] }
function Format-Verdict([bool] $Ok) { if ($Ok) { 'PASS' } else { 'FAIL' } }
function Format-Presenter($r) { if ($r.composed -eq 1) { if ($r.warp -eq 1) { 'composition (WARP)' } else { 'composition' } } else { 'GDI' } }
# Every report of `$Name` named the presenter it should.
$presenterOk = @{}
function Test-Presenter([string] $Name, $r) {
    $ok = $r.composed -eq $targets[$Name].Composed
    if (-not $presenterOk.ContainsKey($Name)) { $presenterOk[$Name] = $true }
    $presenterOk[$Name] = $presenterOk[$Name] -and $ok
}

# U4 startup ---------------------------------------------------------------------------------------
$times = @{}
$last = @{}
foreach ($n in $names) { $times[$n] = [Collections.Generic.List[double]]::new() }
for ($round = 0; $round -le $Rounds; $round++) {
    # Round 0 warms the file cache and is discarded; the order alternates every round.
    for ($k = 0; $k -lt $names.Count; $k++) {
        $n = $names[($k + $round) % $names.Count]
        $r = Invoke-Probe $n 'first-frame'
        Test-Presenter $n $r
        $last[$n] = $r
        if ($round -gt 0) { $times[$n].Add($r.first_frame_us / 1000) }
        Start-Sleep -Milliseconds 300
    }
}
$lines = @("process start to the first painted frame at $Size, median of $Rounds interleaved rounds (gates: each <= 3x bare_window; each run on its presenter)")
foreach ($n in $names) {
    $stats = $times[$n] | Measure-Object -Minimum -Maximum
    $lines += '{0,-6} {1,7:N1} ms  (min {2:N1}, max {3:N1})  {4}' -f $n, (Get-Median $times[$n]), $stats.Minimum, $stats.Maximum, (Format-Presenter $last[$n])
}
$ok = $true
foreach ($n in @('shell', 'glass')) {
    $ratio = (Get-Median $times[$n]) / (Get-Median $times.bare)
    $want = if ($targets[$n].Composed -eq 1) { 'composition' } else { 'GDI' }
    $lines += '{0,-6} ratio {1:N2}  {2}; presenter {3} in every run  {4}' -f $n, $ratio, (Format-Verdict ($ratio -le 3)), $want, (Format-Verdict $presenterOk[$n])
    $ok = $ok -and $ratio -le 3 -and $presenterOk[$n]
}
Publish 'U4 startup' $lines $ok
if (-not $ok) { $failed++ }

# U6, U7 idle --------------------------------------------------------------------------------------
$idle = @{}
foreach ($n in $names) {
    $idle[$n] = Invoke-Probe $n 'idle'
    Test-Presenter $n $idle[$n]
    Start-Sleep -Milliseconds 300
}
$lines = @("idle at ${Size}: 3 s to settle after the first frame, then 5 s untouched (gates: U6 no timer and no paint; U7 working set <= 40 MiB; each on its presenter)")
foreach ($n in $names) {
    $r = $idle[$n]
    $lines += '{0,-6} CPU {1,6:N1} ms  messages {2}  timers {3}  paints {4}  working set {5,5:N1} MiB  private {6,5:N1} MiB  {7}' -f `
        $n, ($r.idle_cpu_us / 1000), $r.messages, $r.timers, $r.paints, ($r.working_set_kib / 1024), ($r.private_kib / 1024), (Format-Presenter $r)
}
$ok = $true
foreach ($n in @('shell', 'glass')) {
    $r = $idle[$n]
    $quiet = $r.timers -eq 0 -and $r.paints -eq 0
    $small = $r.working_set_kib -le 40 * 1024
    $lines += '{0,-6} U6 {1}  U7 {2}  presenter {3}' -f $n, (Format-Verdict $quiet), (Format-Verdict $small), (Format-Verdict $presenterOk[$n])
    # CPU time is charged in clock ticks (15.6 ms), so one tick can be noise.
    if ($r.idle_cpu_us -gt [Math]::Max($idle.bare.idle_cpu_us, 15625)) {
        $lines += "advisory: $n used more CPU while idle than bare_window and one clock tick"
    }
    $ok = $ok -and $quiet -and $small -and $presenterOk[$n]
}
Publish 'U6, U7 idle' $lines $ok
if (-not $ok) { $failed++ }

# U12 UI Automation --------------------------------------------------------------------------------
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class UiaGateWin32 {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")]
    public static extern bool PostMessageW(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp);
}
'@
$ae = [Windows.Automation.AutomationElement]
$types = [Windows.Automation.ControlType]

# Every element of `$Type` (and `$Name`, if given) under `$Root`.
function Find-Elements($Root, $Type, [string] $Name) {
    $c = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $Type)
    if ($Name) {
        $c = [Windows.Automation.AndCondition]::new($c, [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $Name))
    }
    @($Root.FindAll([Windows.Automation.TreeScope]::Descendants, $c))
}

# The table's rows: the DataItems right under the DataGrid "Programs" (TD-0009 A2.5). Its rows'
# view isn't exposed and its header is a Group, so cells, deeper down, aren't among them. Only the
# rows built are there: those in view, half a view either side, and the current row.
function Get-Rows($Root) {
    $c = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $types::DataItem)
    @(Find-Elements $Root $types::DataGrid 'Programs' | ForEach-Object { @($_.FindAll([Windows.Automation.TreeScope]::Children, $c)) })
}

# Polls `$Test` every 10 ms until it's true or `$Ms` pass; returns whether it came true.
function Wait-Until([scriptblock] $Test, [int] $Ms = 3000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do {
        if (& $Test) { return $true }
        Start-Sleep -Milliseconds 10
    } while ($sw.ElapsedMilliseconds -lt $Ms)
    $false
}

function Get-Pattern($Element, $Pattern) {
    $p = $null
    if (-not $Element.TryGetCurrentPattern($Pattern, [ref] $p)) { throw "'$($Element.Current.Name)' has no $($Pattern.ProgrammaticName)" }
    $p
}

$lines = @("UI Automation on the shell at $Size (advisory: tree served <= 100 ms after the window shows; the checks are gates)")
$checks = [ordered]@{}
# This process's first UIA calls load the client (UIAutomationCore, the managed client, these
# functions' first runs): 0.5 to 3.8 s on runners, which U12 must not charge to the shell. The same
# calls run first on bare_window, which serves next to nothing; that time is reported on its own.
$warm = Start-Process -FilePath $exe.bare -PassThru
try {
    $warmed = Wait-Until { $warm.Refresh(); $warm.MainWindowHandle -ne [IntPtr]::Zero } 30000
    if ($warmed) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $null = Find-Elements ($ae::FromHandle($warm.MainWindowHandle)) $types::Button 'Review changes'
        $lines += 'UIA client warm-up on bare_window: {0:N1} ms (not part of U12)' -f $sw.Elapsed.TotalMilliseconds
    } else {
        $lines += 'UIA client warm-up skipped: bare_window showed no window in 30 s'
    }
} finally {
    if (-not $warm.HasExited) {
        if ($warm.MainWindowHandle -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($warm.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $warm.WaitForExit(5000)) { $warm.Kill() }
    }
}
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $hwnd = [IntPtr]::Zero
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $root = $ae::FromHandle($hwnd)
    $review = $null
    $served = Wait-Until {
        $script:review = Find-Elements $root $types::Button 'Review changes' | Select-Object -First 1
        $null -ne $script:review
    } 10000
    $servedMs = $sw.Elapsed.TotalMilliseconds
    $lines += if ($served) { 'tree served {0:N1} ms after the window showed{1}' -f $servedMs, $(if ($servedMs -gt 100) { ' (advisory: over 100 ms)' } else { '' }) } else { 'tree NOT served in 10 s' }
    $checks['served'] = $served

    $pages = @(Find-Elements $root $types::List 'Pages' | ForEach-Object { Find-Elements $_ $types::ListItem })
    $rows = @(Get-Rows $root)
    $boxes = @(Find-Elements $root $types::CheckBox | ForEach-Object { $_.Current.Name })
    $checks['12 pages, Overview first'] = $pages.Count -eq 12 -and $pages[0].Current.Name -eq 'Overview'
    # 750 rows on the first tab. The table builds those in view and half a view either side, and
    # UIA sees each one built: their viewport is a generic container AccessKit leaves out.
    $checks['10 to 99 rows of 750, each named'] = $rows.Count -ge 10 -and $rows.Count -lt 100 -and @($rows | Where-Object { $_.Current.Name }).Count -eq $rows.Count
    $checks['4 check boxes'] = ($boxes -join ',') -eq 'Apps,Games,Tools,System'
    $lines += "pages $($pages.Count), rows $($rows.Count) (first '$(if ($rows) { $rows[0].Current.Name })'), check boxes: $($boxes -join ', ')"

    $toggleState = [Windows.Automation.ToggleState]
    $games = Find-Elements $root $types::CheckBox 'Games' | Select-Object -First 1
    if ($games) {
        $toggle = Get-Pattern $games ([Windows.Automation.TogglePattern]::Pattern)
        $before = $toggle.Current.ToggleState
        $toggle.Toggle()
        $checks['Toggle(Games) turns it on'] = $before -eq $toggleState::Off -and (Wait-Until { $toggle.Current.ToggleState -eq $toggleState::On })
    }
    if ($rows.Count -gt 3) {
        # SetFocus makes it current (TD-0009 A2.5); its SelectionItem comes with slice 5c.
        $rows[3].SetFocus()
        $checks['SetFocus(row 4) focuses it'] = Wait-Until { $rows[3].Current.HasKeyboardFocus }
    }
    if ($review) {
        (Get-Pattern $review ([Windows.Automation.InvokePattern]::Pattern)).Invoke()
        $checks['Invoke(Review changes) opens the drawer'] = Wait-Until {
            (Find-Elements $root $types::Group 'Changes').Count -eq 1 -and (Find-Elements $root $types::Button 'Hide changes').Count -eq 1
        }
    }
} catch {
    $lines += "UI Automation FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE' ; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-44} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'U12 UI Automation' $lines $ok
if (-not $ok) { $failed++ }

# Live theme switch (TD-0009 §5.2, §8) -------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ThemeGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr hwnd, IntPtr hdc);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out RECT r);
    [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr hdc, int x, int y);
    [DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, uint rop);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, IntPtr wp, string lp, uint flags, uint ms, out IntPtr result);

    // The grid points the last Luminance call couldn't read (CLR_INVALID: outside the DC's clip,
    // for example under another window).
    public static int Unreadable;

    // Each grid point's brightness from the last Luminance call, column by column; -1 if unread.
    static readonly double[] Cells = new double[160];

    // The last Luminance call's grid, 10 rows of 16, top row first: each point's brightness as a
    // digit, 0 black to 9 white, or '?' if unread.
    public static string[] Map() {
        var rows = new string[10];
        for (int j = 0; j < 10; j++) {
            var row = new char[16];
            for (int i = 0; i < 16; i++) {
                double l = Cells[i * 10 + j];
                row[i] = l < 0 ? '?' : (char)('0' + (int)Math.Round(l * 9));
            }
            rows[j] = new string(row);
        }
        return rows;
    }

    // The mean brightness (0..1, Rec. 709 weights over sRGB) of the readable points of a 16 x 10
    // grid over the client area; -1 if none is readable.
    public static double Luminance(IntPtr hwnd) {
        Unreadable = 0;
        for (int k = 0; k < Cells.Length; k++) Cells[k] = -1;
        RECT r;
        if (!GetClientRect(hwnd, out r) || r.Right < 16 || r.Bottom < 10) return -1;
        IntPtr dc = GetDC(hwnd);
        if (dc == IntPtr.Zero) return -1;
        double sum = 0;
        int n = 0;
        try {
            for (int i = 0; i < 16; i++) {
                for (int j = 0; j < 10; j++) {
                    uint c = GetPixel(dc, (2 * i + 1) * r.Right / 32, (2 * j + 1) * r.Bottom / 20);
                    if (c == 0xFFFFFFFF) { Unreadable++; continue; }
                    double l = 0.2126 * ((c & 0xFF) / 255.0) + 0.7152 * (((c >> 8) & 0xFF) / 255.0) + 0.0722 * (((c >> 16) & 0xFF) / 255.0);
                    Cells[i * 10 + j] = l;
                    sum += l;
                    n++;
                }
            }
        } finally { ReleaseDC(hwnd, dc); }
        return n == 0 ? -1 : sum / n;
    }
}
'@
# The screen around `$Rect` (left, top, right, bottom), DWM's shadow and corners included, saved
# to -Out as `$Name` for A1.3's check by hand. Returns a report line; a failure is reported, not
# gated.
function Save-Screen([int[]] $Rect, [string] $Name, [int] $Margin = 16) {
    $bmp = $null
    $g = $null
    try {
        Add-Type -AssemblyName System.Drawing
        $x = $Rect[0] - $Margin
        $y = $Rect[1] - $Margin
        $w = $Rect[2] - $Rect[0] + 2 * $Margin
        $h = $Rect[3] - $Rect[1] + 2 * $Margin
        $bmp = [Drawing.Bitmap]::new($w, $h)
        $g = [Drawing.Graphics]::FromImage($bmp)
        # BitBlt itself: CopyFromScreen rejects SRCCOPY | CAPTUREBLT as an undefined enum value.
        $dst = $g.GetHdc()
        $screen = [ThemeGateWin32]::GetDC([IntPtr]::Zero)
        try {
            if (-not [ThemeGateWin32]::BitBlt($dst, 0, 0, $w, $h, $screen, $x, $y, 0x40CC0020)) { throw 'BitBlt failed' } # SRCCOPY | CAPTUREBLT
        } finally {
            [void][ThemeGateWin32]::ReleaseDC([IntPtr]::Zero, $screen)
            $g.ReleaseHdc($dst)
        }
        $bmp.Save((Join-Path $Out $Name), [Drawing.Imaging.ImageFormat]::Png)
        "  screenshot $Name ($($Rect[2] - $Rect[0])x$($Rect[3] - $Rect[1]) at $($Rect[0]),$($Rect[1]), $Margin px around)"
    } catch {
        "  screenshot $Name failed: $($_.Exception.Message)"
    } finally {
        if ($g) { $g.Dispose() }
        if ($bmp) { $bmp.Dispose() }
    }
}

# The window follows Windows' app mode live: the runner's `AppsUseLightTheme` is set to dark and
# `WM_SETTINGCHANGE "ImmersiveColorSet"` sent to the shell's window (as Settings broadcasts it), then
# back to light. Gate: the client area turns dark, then light again, within 2 s each; the time is
# advisory (one frame is the design, TD-0009 §8). The runner's value is restored.
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$hadKey = Test-Path $personalize
$was = if ($hadKey) { (Get-ItemProperty $personalize -ErrorAction SilentlyContinue).AppsUseLightTheme } else { $null }
function Set-AppMode([int] $Light, [IntPtr] $Hwnd) {
    if (-not (Test-Path $personalize)) { $null = New-Item -Path $personalize -Force }
    Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value $Light -Type DWord
    $result = [IntPtr]::Zero
    # SMTO_ABORTIFHUNG; our window only, so the runner's other windows aren't involved. Zero: the
    # window was hung, or didn't answer in 2 s.
    [ThemeGateWin32]::SendMessageTimeoutW($Hwnd, 0x001A, [IntPtr]::Zero, 'ImmersiveColorSet', 2, 2000, [ref] $result) -ne [IntPtr]::Zero
}
$lines = @("live theme switch on the shell at $Size, by mean brightness (gates: dark, then light again, each within 2 s; advisory: <= 100 ms)")
$checks = [ordered]@{}
$p = $null
$hwnd = [IntPtr]::Zero
try {
    Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value 1 -Type DWord -ErrorAction SilentlyContinue
    # The pointer rests in the screen's corner, off the window: a pointer resting on one of the
    # shell's tooltip anchors would show a tip over the sampled points.
    [void][ThemeGateWin32]::SetCursorPos(0, 0)
    $p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
    $null = $p.Handle
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $script:startLum = -1
    $checks['starts light'] = Wait-Until { $script:startLum = [ThemeGateWin32]::Luminance($hwnd); $script:startLum -gt 0.5 } 5000
    $lines += '{0,-12} {1,7:N1} ms  luminance {2:N2}{3}' -f 'light', $sw.Elapsed.TotalMilliseconds, $script:startLum, $(if ([ThemeGateWin32]::Unreadable -gt 0) { " ($([ThemeGateWin32]::Unreadable) of 160 points unreadable)" } else { '' })
    if (-not $checks['starts light']) {
        # What the window was doing (an intermittent failure, TD-0009 §13, run 45134227331: 0.29
        # for 5 s, then the switches passed). Read before anything changes it.
        $cr = New-Object ThemeGateWin32+RECT
        [void][ThemeGateWin32]::GetClientRect($hwnd, [ref] $cr)
        $lines += "  client $($cr.Right)x$($cr.Bottom); hung: $([ThemeGateWin32]::IsHungAppWindow($hwnd)); foreground: $([ThemeGateWin32]::GetForegroundWindow() -eq $hwnd); process responding: $(-not $p.HasExited -and $p.Responding)"
        $lines += '  the grid, top row first (0 black to 9 white, ? unread):'
        $lines += [ThemeGateWin32]::Map() | ForEach-Object { "    $_" }
        $wr = New-Object ThemeGateWin32+RECT
        if ([ThemeGateWin32]::GetWindowRect($hwnd, [ref] $wr)) { $lines += Save-Screen @($wr.Left, $wr.Top, $wr.Right, $wr.Bottom) 'theme-start.png' }
    }
    foreach ($step in @(@{ Name = 'dark'; Light = 0 }, @{ Name = 'light again'; Light = 1 })) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $delivered = Set-AppMode $step.Light $hwnd
        $switched = Wait-Until {
            $l = [ThemeGateWin32]::Luminance($hwnd)
            $l -ge 0 -and (($step.Light -eq 0 -and $l -lt 0.5) -or ($step.Light -eq 1 -and $l -gt 0.5))
        } 2000
        $ms = $sw.Elapsed.TotalMilliseconds
        $lines += '{0,-12} {1}  luminance {2:N2}{3}{4}' -f $step.Name, $(if ($switched) { '{0,7:N1} ms' -f $ms } else { 'NOT in 2 s' }), [ThemeGateWin32]::Luminance($hwnd), $(if ($switched -and $ms -gt 100) { ' (advisory: over 100 ms)' } else { '' }), $(if (-not $delivered) { ' (WM_SETTINGCHANGE not answered: hung or over 2 s)' } else { '' })
        $checks["turns $($step.Name)"] = $switched
    }
} catch {
    $lines += "theme switch FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if ($null -ne $was) {
        Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value $was -Type DWord
    } elseif ($hadKey) {
        Remove-ItemProperty -Path $personalize -Name AppsUseLightTheme -ErrorAction SilentlyContinue
    } else {
        Remove-Item -Path $personalize -ErrorAction SilentlyContinue
    }
    if ($p -and -not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-44} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Live theme switch' $lines $ok
if (-not $ok) { $failed++ }

# Glass (ADR-0007, TD-0009 §5.2, §10) --------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Threading;
public static class GlassGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd, ref POINT p);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr hwnd, IntPtr hdc);
    [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr hdc, int x, int y);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);

    // What the screen shows (DWM's composition, 0x00BBGGRR) at client points given at 96 DPI;
    // 0xFFFFFFFF where unreadable.
    public static uint[] Sample(IntPtr hwnd, int[] xs, int[] ys) {
        uint dpi = GetDpiForWindow(hwnd);
        if (dpi == 0) dpi = 96;
        var colors = new uint[xs.Length];
        IntPtr dc = GetDC(IntPtr.Zero);
        if (dc == IntPtr.Zero) { for (int i = 0; i < colors.Length; i++) colors[i] = 0xFFFFFFFF; return colors; }
        try {
            for (int i = 0; i < xs.Length; i++) {
                var p = new POINT { X = (int)(xs[i] * dpi / 96), Y = (int)(ys[i] * dpi / 96) };
                colors[i] = ClientToScreen(hwnd, ref p) ? GetPixel(dc, p.X, p.Y) : 0xFFFFFFFF;
            }
        } finally { ReleaseDC(IntPtr.Zero, dc); }
        return colors;
    }

    // Makes the window the active one: DWM draws an inactive window's backdrop as a plain color
    // (S-01). The foreground lock refuses a process without recent input, so an Alt press comes
    // first, as S-01's G1 did.
    public static bool Activate(IntPtr hwnd) {
        for (int i = 0; i < 5 && GetForegroundWindow() != hwnd; i++) {
            keybd_event(0x12, 0, 0, UIntPtr.Zero);
            keybd_event(0x12, 0, 2, UIntPtr.Zero);
            SetForegroundWindow(hwnd);
            Thread.Sleep(300);
        }
        return GetForegroundWindow() == hwnd;
    }
}
'@
# The glass shell's window base, read off the screen at three points that show only the base: the
# header's padding above and below its row, and the content's left padding (client px at 100 %,
# 1000x700). Glass shows DWM's backdrop there; solid shows the light theme's bg.app token.
# Transparency is then switched off and on again (registry, then WM_SETTINGCHANGE
# "ImmersiveColorSet" to our window, as for the theme): the base turns to the token and back, live.
# --materials glass forces glass over the GPU, remote and battery saver, but not over transparency,
# so the switch reaches it. The runner's values are restored.
$baseXs = [int[]] @(300, 700, 207)
$baseYs = [int[]] @(3, 45, 400)
$token = @(0xF6, 0xF7, 0xF9) # Theme::LIGHT's bg.app
function ConvertTo-Rgb([uint32] $c) { , @([int] ($c -band 0xFF), [int] (($c -shr 8) -band 0xFF), [int] (($c -shr 16) -band 0xFF)) }
function Get-Distance($c, $want) { [Math]::Max([Math]::Max([Math]::Abs($c[0] - $want[0]), [Math]::Abs($c[1] - $want[1])), [Math]::Abs($c[2] - $want[2])) }
function Format-Rgb($c) { 'rgb({0},{1},{2})' -f $c[0], $c[1], $c[2] }
function Get-Base([IntPtr] $Hwnd) { @([GlassGateWin32]::Sample($Hwnd, $baseXs, $baseYs) | ForEach-Object { , (ConvertTo-Rgb $_) }) }
# Every point within 3 of `$Want`'s point (one color, or one per point).
function Test-Near($Base, $Want) {
    for ($i = 0; $i -lt $Base.Count; $i++) {
        $w = if ($Want[0] -is [array]) { $Want[$i] } else { $Want }
        if ((Get-Distance $Base[$i] $w) -gt 3) { return $false }
    }
    $true
}
function Set-Transparency([int] $On, [IntPtr] $Hwnd) {
    if (-not (Test-Path $personalize)) { $null = New-Item -Path $personalize -Force }
    Set-ItemProperty -Path $personalize -Name EnableTransparency -Value $On -Type DWord
    $result = [IntPtr]::Zero
    [void][ThemeGateWin32]::SendMessageTimeoutW($Hwnd, 0x001A, [IntPtr]::Zero, 'ImmersiveColorSet', 2, 2000, [ref] $result)
}
$hadKey = Test-Path $personalize
$saved = @{}
foreach ($v in 'AppsUseLightTheme', 'EnableTransparency') {
    $saved[$v] = if ($hadKey) { (Get-ItemProperty $personalize -ErrorAction SilentlyContinue).$v } else { $null }
}
$lines = @("glass shell at $Size, light, active; window base at client ($((0..($baseXs.Count - 1) | ForEach-Object { "$($baseXs[$_]),$($baseYs[$_])" }) -join '; ')) read off the screen (gates: not the solid token while glass; the token with transparency off, and the backdrop again with it on, each within 2 s; advisory: <= 100 ms)")
$checks = [ordered]@{}
$p = $null
$hwnd = [IntPtr]::Zero
try {
    if (-not $hadKey) { $null = New-Item -Path $personalize -Force }
    Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value 1 -Type DWord
    Set-ItemProperty -Path $personalize -Name EnableTransparency -Value 1 -Type DWord
    $p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size, '--materials', 'glass') -PassThru
    $null = $p.Handle
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the glass shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $active = [GlassGateWin32]::Activate($hwnd)
    Start-Sleep -Milliseconds 1500 # the first frames, and DWM's backdrop transition
    $glass = Get-Base $hwnd
    $lines += 'glass        {0}{1}' -f (($glass | ForEach-Object { Format-Rgb $_ }) -join '; '), $(if ($active) { '' } else { '  (NOT the active window)' })
    $lit = @($glass | Where-Object { (0.2126 * $_[0] + 0.7152 * $_[1] + 0.0722 * $_[2]) / 255 -gt 0.5 }).Count -eq $glass.Count
    $checks['glass: the backdrop, not the solid token'] = $lit -and @($glass | Where-Object { (Get-Distance $_ $token) -ge 3 }).Count -eq $glass.Count
    foreach ($step in @(@{ Name = 'transparency off'; On = 0 }, @{ Name = 'transparency on'; On = 1 })) {
        $want = if ($step.On -eq 0) { $token } else { $glass }
        $sw = [Diagnostics.Stopwatch]::StartNew()
        Set-Transparency $step.On $hwnd
        $switched = Wait-Until { Test-Near (Get-Base $hwnd) $want } 2000
        $ms = $sw.Elapsed.TotalMilliseconds
        $now = Get-Base $hwnd
        $lines += '{0,-12} {1}  {2}{3}' -f $step.Name, $(if ($switched) { '{0,7:N1} ms' -f $ms } else { 'NOT in 2 s' }), (($now | ForEach-Object { Format-Rgb $_ }) -join '; '), $(if ($switched -and $ms -gt 100) { ' (advisory: over 100 ms)' } else { '' })
        $checks[$(if ($step.On -eq 0) { 'transparency off: the solid token, live' } else { 'transparency on: the backdrop again, live' })] = $switched
    }
} catch {
    $lines += "glass FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    foreach ($v in $saved.Keys) {
        if ($null -ne $saved[$v]) {
            Set-ItemProperty -Path $personalize -Name $v -Value $saved[$v] -Type DWord
        } else {
            Remove-ItemProperty -Path $personalize -Name $v -ErrorAction SilentlyContinue
        }
    }
    if (-not $hadKey) { Remove-Item -Path $personalize -ErrorAction SilentlyContinue }
    if ($p -and -not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the glass shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-44} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Glass' $lines $ok
if (-not $ok) { $failed++ }

# Text input (TD-0009 §11 step 3) ------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TextGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public InputUnion u; }
    [StructLayout(LayoutKind.Sequential)]
    struct CURSORINFO { public int cbSize; public int flags; public IntPtr hCursor; public int x; public int y; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")]
    static extern bool GetCursorInfo(ref CURSORINFO info);
    [DllImport("user32.dll")]
    static extern IntPtr LoadCursorW(IntPtr instance, IntPtr id);
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);
    const uint KEYBOARD = 1, KEYUP = 2, UNICODE = 4;
    static INPUT Key(ushort vk, ushort scan, uint flags) {
        var i = new INPUT { type = KEYBOARD };
        i.u.ki = new KEYBDINPUT { wVk = vk, wScan = scan, dwFlags = flags };
        return i;
    }
    static uint Send(INPUT[] inputs) { return SendInput((uint) inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT))); }
    // `text` as Unicode key events, a down and an up per UTF-16 unit: a character outside the BMP
    // arrives as its two surrogates, in two WM_CHARs. Returns the events sent.
    public static uint Type(string text) {
        var inputs = new INPUT[text.Length * 2];
        for (int k = 0; k < text.Length; k++) {
            inputs[2 * k] = Key(0, text[k], UNICODE);
            inputs[2 * k + 1] = Key(0, text[k], UNICODE | KEYUP);
        }
        return Send(inputs);
    }
    // `vk` pressed and released with `mods` held. Returns the events sent.
    public static uint Chord(ushort[] mods, ushort vk) {
        var inputs = new INPUT[mods.Length * 2 + 2];
        int n = 0;
        foreach (var m in mods) { inputs[n++] = Key(m, 0, 0); }
        inputs[n++] = Key(vk, 0, 0);
        inputs[n++] = Key(vk, 0, KEYUP);
        for (int k = mods.Length - 1; k >= 0; k--) { inputs[n++] = Key(mods[k], 0, KEYUP); }
        return Send(inputs);
    }
    [StructLayout(LayoutKind.Sequential)]
    struct POINT { public int x; public int y; }
    [DllImport("user32.dll")]
    static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")]
    static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")]
    static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr hwnd, System.Text.StringBuilder name, int size);
    [DllImport("kernel32.dll")]
    static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")]
    static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("user32.dll")]
    static extern IntPtr GetCursor();
    [DllImport("user32.dll")]
    static extern int GetSystemMetrics(int index);
    const uint MOUSE = 0, MOVE = 1, ABSOLUTE = 0x8000;
    // The system cursors, shared handles: the same in every process of the session.
    static readonly IntPtr IBeam = LoadCursorW(IntPtr.Zero, (IntPtr) 32513);
    static readonly IntPtr Arrow = LoadCursorW(IntPtr.Zero, (IntPtr) 32512);
    static readonly IntPtr SizeWE = LoadCursorW(IntPtr.Zero, (IntPtr) 32644);
    static string Name(IntPtr cursor) {
        if (cursor == IntPtr.Zero) { return "none"; }
        if (cursor == IBeam) { return "I-beam"; }
        if (cursor == Arrow) { return "arrow"; }
        if (cursor == SizeWE) { return "left-right arrows"; }
        return "0x" + cursor.ToString("X");
    }
    // The pointer moved to (x, y) on the primary monitor as mouse input, like a mouse would, not
    // placed (SetCursorPos). Returns the events sent.
    public static uint MoveTo(int x, int y) {
        int w = Math.Max(2, GetSystemMetrics(0)), h = Math.Max(2, GetSystemMetrics(1)); // SM_CXSCREEN, SM_CYSCREEN
        var i = new INPUT { type = MOUSE };
        i.u.mi = new MOUSEINPUT { dx = (int) (x * 65535L / (w - 1)), dy = (int) (y * 65535L / (h - 1)), dwFlags = MOVE | ABSOLUTE };
        return Send(new[] { i });
    }
    // Whether the pointer over `hwnd` is an I-beam, and what was read.
    public static string Pointer(IntPtr hwnd, out bool ibeam) { return Pointer(hwnd, 32513, out ibeam); }
    // Whether the pointer over `hwnd` is the system cursor `id` (IDC_*), and what was read. A
    // pointer Windows shows must be that one. When it shows none (no mouse on the machine, or
    // hidden while typing), the cursor the window's thread set is read instead, through its input
    // queue (AttachThreadInput, then GetCursor): what the shell asked for is all there is to see.
    public static string Pointer(IntPtr hwnd, int id, out bool match) {
        IntPtr want = LoadCursorW(IntPtr.Zero, (IntPtr) id);
        var info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(info);
        bool got = GetCursorInfo(ref info);
        bool shown = got && (info.flags & 1) != 0; // CURSOR_SHOWING
        uint pid;
        uint thread = GetWindowThreadProcessId(hwnd, out pid);
        IntPtr set = IntPtr.Zero;
        uint me = GetCurrentThreadId();
        bool attached = thread != 0 && thread != me && AttachThreadInput(me, thread, true);
        if (attached) {
            set = GetCursor();
            AttachThreadInput(me, thread, false);
        }
        match = shown ? info.hCursor == want : attached && set == want;
        POINT at;
        GetCursorPos(out at);
        IntPtr under = GetAncestor(WindowFromPoint(at), 2); // GA_ROOT
        uint underPid;
        GetWindowThreadProcessId(under, out underPid);
        var cls = new System.Text.StringBuilder(64);
        GetClassNameW(under, cls, cls.Capacity);
        string owner = under == hwnd ? "the shell" : "another window (" + cls + ", pid " + underPid + ")";
        return String.Format(
            "shown: {0} (GetCursorInfo {1}, flags {2}); set by the shell: {3}; at ({4}, {5}) over {6}; mouse present: {7}",
            shown ? Name(info.hCursor) : "no pointer", got ? "ok" : "failed", info.flags,
            attached ? Name(set) : "unread", at.x, at.y, owner, GetSystemMetrics(19)); // SM_MOUSEPRESENT
    }
}
'@
$ctrl = [uint16[]] @(0x11)
$shift = [uint16[]] @(0x10)
$none = [uint16[]] @()

$lines = @("Text input in the shell's filter field at $Size (advisory: typing reaches UIA's value <= 50 ms a key; the checks are gates)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$savedClip = Get-Clipboard -Raw -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $edit = $null
    $found = Wait-Until {
        $script:edit = Find-Elements $root $types::Edit 'Filter rows' | Select-Object -First 1
        $null -ne $script:edit
    } 10000
    $checks['an Edit named "Filter rows"'] = $found
    if (-not $found) { throw 'no Edit named "Filter rows" in 10 s' }
    $value = Get-Pattern $edit ([Windows.Automation.ValuePattern]::Pattern)
    $checks['empty, its placeholder as help text'] = $value.Current.Value -eq '' -and $edit.Current.HelpText -eq 'Filter'
    [void][TextGateWin32]::SetForegroundWindow($hwnd)
    $edit.SetFocus()
    $checks['SetFocus gives it keyboard focus'] = Wait-Until { $edit.Current.HasKeyboardFocus }

    # Typed, with a character outside the BMP (two WM_CHARs).
    $typed = 'contoso ' + [char]::ConvertFromUtf32(0x1F600)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $sent = [TextGateWin32]::Type($typed)
    $got = Wait-Until { $value.Current.Value -eq $typed } 2000
    $ms = $sw.Elapsed.TotalMilliseconds
    $lines += if ($got) {
        '{0} UTF-16 units typed ({1} events), in UIA''s value after {2:N1} ms{3}' -f $typed.Length, $sent, $ms, $(if ($ms / $typed.Length -gt 50) { ' (advisory: over 50 ms a key)' } else { '' })
    } else { "typed '$typed', UIA's value is '$($value.Current.Value)'" }
    $checks['typing, a surrogate pair included'] = $got

    Set-Clipboard -Value 'before'
    [void][TextGateWin32]::Chord($ctrl, 0x41) # Ctrl+A
    [void][TextGateWin32]::Chord($ctrl, 0x43) # Ctrl+C
    $checks['Ctrl+A, Ctrl+C copy it'] = Wait-Until { (Get-Clipboard -Raw) -eq $typed } 2000
    [void][TextGateWin32]::Chord($none, 0x08) # Backspace
    $checks['Backspace deletes the selection'] = Wait-Until { $value.Current.Value -eq '' } 2000

    Set-Clipboard -Value "C:\Program Files`r`nsecond line"
    [void][TextGateWin32]::Chord($ctrl, 0x56) # Ctrl+V
    $pasted = 'C:\Program Filessecond line'
    $checks['Ctrl+V pastes two lines as one'] = Wait-Until { $value.Current.Value -eq $pasted } 2000
    [void][TextGateWin32]::Chord($shift, 0x24) # Shift+Home
    [void][TextGateWin32]::Chord($ctrl, 0x58) # Ctrl+X
    $checks['Shift+Home, Ctrl+X cut it'] = Wait-Until { $value.Current.Value -eq '' -and (Get-Clipboard -Raw) -eq $pasted } 2000

    $value.SetValue('Fabrikam')
    $checks['SetValue("Fabrikam") filters the table'] = Wait-Until {
        $rows = @(Get-Rows $root)
        $value.Current.Value -eq 'Fabrikam' -and $rows.Count -gt 0 -and $rows[0].Current.Name -like 'Fabrikam*'
    } 2000

    # The pointer comes from the table into the field as mouse input, so the window sees the move
    # into it; what it shows (or sets) there is the check, and both places are reported.
    $win = $root.Current.BoundingRectangle
    $r = $edit.Current.BoundingRectangle
    $hit = $false
    $off = [TextGateWin32]::MoveTo([int] ($win.X + $win.Width / 2), [int] ($win.Y + $win.Height * 3 / 4))
    $null = Wait-Until { $null = [TextGateWin32]::Pointer($hwnd, [ref] $script:hit); -not $script:hit } 1000
    $lines += "pointer off the field ($off sent): $([TextGateWin32]::Pointer($hwnd, [ref] $hit))"
    $fx = [int] ($r.X + $r.Width / 2)
    $fy = [int] ($r.Y + $r.Height / 2)
    $on = [TextGateWin32]::MoveTo($fx, $fy)
    $pointer = ''
    $checks['an I-beam over it'] = Wait-Until {
        $script:pointer = [TextGateWin32]::Pointer($hwnd, [ref] $script:hit)
        $script:hit
    } 2000
    $lines += "pointer on the field at ($fx, $fy) ($on sent): $pointer"
} catch {
    $lines += "text input FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if ($null -ne $savedClip) { Set-Clipboard -Value $savedClip }
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-44} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Text input' $lines $ok
if (-not $ok) { $failed++ }

# Controls (TD-0009 §11 step 4) --------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ControlsGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public InputUnion u; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
    // `vk` going down, or up: one event, so a held key is held until its up. Returns the events
    // sent.
    public static uint Key(ushort vk, bool down) {
        var i = new INPUT { type = 1 };
        i.u.ki = new KEYBDINPUT { wVk = vk, dwFlags = down ? 0u : 2u };
        return SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@
function Send-Key([uint16] $Vk) {
    [void][ControlsGateWin32]::Key($Vk, $true)
    [void][ControlsGateWin32]::Key($Vk, $false)
}
function Test-Selected($Element) { (Get-Pattern $Element ([Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected }
function Select-Item($Element) { (Get-Pattern $Element ([Windows.Automation.SelectionItemPattern]::Pattern)).Select() }
function Find-First($Root, $Type, [string] $Name) { Find-Elements $Root $Type $Name | Select-Object -First 1 }

$lines = @("Controls in the shell at $Size (the checks are gates; the hold's time is gated at >= 1,500 ms, its excess advisory)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $store = $null
    $found = Wait-Until { $script:store = Find-First $root $types::TabItem 'Store apps'; $null -ne $script:store } 10000
    $checks['TabItems in a Tab "Program types"'] = $found -and (Find-Elements $root $types::Tab 'Program types').Count -eq 1
    if (-not $found) { throw 'no TabItem "Store apps" in 10 s' }
    $desktop = Find-First $root $types::TabItem 'Desktop programs'
    $firstRow = { @(Get-Rows $root | ForEach-Object { $_.Current.Name }) | Select-Object -First 1 }

    # Tabs: Select through UIA, then Left on the focused tab (selection follows focus).
    Select-Item $store
    $checks['Select(Store apps) picks it, the table follows'] = Wait-Until { (Test-Selected $store) -and -not (Test-Selected $desktop) -and (& $firstRow) -like 'Tailspin*' } 2000
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)
    $store.SetFocus()
    $checks['Left on it picks Desktop programs'] = (Wait-Until { $store.Current.HasKeyboardFocus } 2000) -and $(Send-Key 0x25; Wait-Until { (Test-Selected $desktop) -and $desktop.Current.HasKeyboardFocus -and (& $firstRow) -like 'Contoso*' } 2000)

    # The rail: a list, so the arrows move focus only, and Enter opens.
    $overview = Find-First $root $types::ListItem 'Overview'
    $clean = Find-First $root $types::ListItem 'Clean'
    $overview.SetFocus()
    $checks['Down on the rail moves focus, not the page'] = (Wait-Until { $overview.Current.HasKeyboardFocus } 2000) -and $(Send-Key 0x28; Wait-Until { $clean.Current.HasKeyboardFocus -and (Test-Selected $overview) -and -not (Test-Selected $clean) } 2000)
    Send-Key 0x0D
    $checks['Enter opens the focused page'] = Wait-Until { (Test-Selected $clean) -and (Find-Elements $root $types::Group 'Clean').Count -eq 1 } 2000

    # Settings' radio group.
    Select-Item (Find-First $root $types::ListItem 'Settings')
    $dark = $null
    $found = Wait-Until { $script:dark = Find-First $root $types::RadioButton 'Dark'; $null -ne $script:dark } 2000
    $checks['3 RadioButtons in a group "Theme"'] = $found -and (Find-Elements $root $types::RadioButton).Count -eq 3 -and (Find-Elements $root $types::Group 'Theme').Count -eq 1
    if ($found) {
        $system = Find-First $root $types::RadioButton 'System'
        Select-Item $dark
        $checks['Select(Dark) picks it'] = Wait-Until { (Test-Selected $dark) -and -not (Test-Selected $system) } 2000
        $dark.SetFocus()
        $checks['Down from the last picks the first'] = (Wait-Until { $dark.Current.HasKeyboardFocus } 2000) -and $(Send-Key 0x28; Wait-Until { (Test-Selected $system) -and $system.Current.HasKeyboardFocus -and -not (Test-Selected $dark) } 2000)
    }

    # The drawer: its progress bar and Hold to apply.
    (Get-Pattern (Find-First $root $types::Button 'Review changes') ([Windows.Automation.InvokePattern]::Pattern)).Invoke()
    $bar = $null
    $found = Wait-Until { $script:bar = Find-First $root $types::ProgressBar 'Applying'; $null -ne $script:bar } 2000
    if (-not $found) { throw 'no ProgressBar "Applying" in the drawer in 2 s' }
    $range = Get-Pattern $bar ([Windows.Automation.RangeValuePattern]::Pattern)
    $checks['a read-only ProgressBar "Applying" at 0 of 100'] = $range.Current.Value -eq 0 -and $range.Current.Maximum -eq 100 -and $range.Current.IsReadOnly
    $hold = Find-First $root $types::Button 'Hold to apply'
    (Get-Pattern $hold ([Windows.Automation.InvokePattern]::Pattern)).Invoke()
    $checks['Invoke(Hold to apply) asks to confirm instead'] = (Wait-Until { (Find-Elements $root $types::Text 'Type 20 to confirm').Count -eq 1 } 2000) -and $range.Current.Value -eq 0
    [void][GlassGateWin32]::Activate($hwnd)
    $hold.SetFocus()
    $focused = Wait-Until { $hold.Current.HasKeyboardFocus } 2000
    [void][ControlsGateWin32]::Key(0x0D, $true)
    Start-Sleep -Milliseconds 500
    [void][ControlsGateWin32]::Key(0x0D, $false)
    Start-Sleep -Milliseconds 300
    $checks['Enter held 0.5 s and let go applies nothing'] = $focused -and $range.Current.Value -eq 0 -and (Find-Elements $root $types::Text 'Applied').Count -eq 0
    $sw = [Diagnostics.Stopwatch]::StartNew()
    [void][ControlsGateWin32]::Key(0x0D, $true)
    $applied = Wait-Until { $range.Current.Value -eq 100 } 4000
    $ms = $sw.Elapsed.TotalMilliseconds
    [void][ControlsGateWin32]::Key(0x0D, $false)
    $lines += if ($applied) { 'Enter held: applied {0:N0} ms after the press{1}' -f $ms, $(if ($ms -gt 1700) { ' (advisory: over 1,700 ms)' } else { '' }) } else { 'Enter held 4 s: nothing applied' }
    $checks['Enter held applies, not before 1,500 ms'] = $applied -and $ms -ge 1500 -and (Wait-Until { (Find-Elements $root $types::Text 'Applied').Count -eq 1 } 2000)
} catch {
    $lines += "controls FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-48} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Controls' $lines $ok
if (-not $ok) { $failed++ }

# Text scaling (TD-0009 §12 item 7) ----------------------------------------------------------------
# Windows' text size is HKCU\Software\Microsoft\Accessibility\TextScaleFactor (100-225). It's
# written here with no broadcast at all: the shell must notice the value itself.
function Get-UISettingsTextScale {
    # UISettings is WinRT, which pwsh 7 can't project; Windows PowerShell 5.1 can.
    $read = '[void][Windows.UI.ViewManagement.UISettings,Windows.UI.ViewManagement,ContentType=WindowsRuntime]; (New-Object Windows.UI.ViewManagement.UISettings).TextScaleFactor'
    try { (& powershell.exe -NoProfile -NonInteractive -Command $read 2>&1 | Out-String).Trim() } catch { "unreadable: $($_.Exception.Message)" }
}

$a11yKey = 'HKCU:\Software\Microsoft\Accessibility'
$hadA11yKey = Test-Path $a11yKey
$savedScale = if ($hadA11yKey) { (Get-ItemProperty -Path $a11yKey -Name TextScaleFactor -ErrorAction SilentlyContinue).TextScaleFactor } else { $null }
$lines = @("Text scaling on the shell at ${Size}: TextScaleFactor written with no broadcast (gates: the title's height x2.25 within 2 s, and back within 2 s; advisory: <= 100 ms)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = $null
try {
    if (-not $hadA11yKey) { $null = New-Item -Path $a11yKey -Force }
    Set-ItemProperty -Path $a11yKey -Name TextScaleFactor -Value 100 -Type DWord
    $p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
    $null = $p.Handle
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $title = $null
    $found = Wait-Until {
        $script:title = Find-Elements $root $types::Text 'DISM Pro' | Select-Object -First 1
        $null -ne $script:title -and $script:title.Current.BoundingRectangle.Height -gt 0
    } 10000
    $checks['a Text named "DISM Pro"'] = $found
    if (-not $found) { throw 'no Text named "DISM Pro" in 10 s' }
    $h100 = $title.Current.BoundingRectangle.Height
    $lines += "UISettings.TextScaleFactor at 100: $(Get-UISettingsTextScale)"

    Set-ItemProperty -Path $a11yKey -Name TextScaleFactor -Value 225 -Type DWord
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $grew = Wait-Until { $title.Current.BoundingRectangle.Height -ge $h100 * 2.25 - 2 } 2000
    $ms = $sw.Elapsed.TotalMilliseconds
    $h225 = $title.Current.BoundingRectangle.Height
    $lines += '225 %   {0,6:N1} ms  title {1} px high (at 100 %: {2}){3}' -f $ms, $h225, $h100, $(if ($ms -gt 100) { ' (advisory: over 100 ms)' } else { '' })
    $checks['225 %: the text grows 2.25x, live'] = $grew
    $lines += "UISettings.TextScaleFactor at 225: $(Get-UISettingsTextScale)"

    Set-ItemProperty -Path $a11yKey -Name TextScaleFactor -Value 100 -Type DWord
    $sw.Restart()
    $back = Wait-Until { [math]::Abs($title.Current.BoundingRectangle.Height - $h100) -le 1 } 2000
    $lines += '100 %   {0,6:N1} ms  title {1} px high' -f $sw.Elapsed.TotalMilliseconds, $title.Current.BoundingRectangle.Height
    $checks['back to 100 %: as it was, live'] = $back
} catch {
    $lines += "text scaling FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if ($null -ne $savedScale) {
        Set-ItemProperty -Path $a11yKey -Name TextScaleFactor -Value $savedScale -Type DWord
    } else {
        Remove-ItemProperty -Path $a11yKey -Name TextScaleFactor -ErrorAction SilentlyContinue
    }
    if (-not $hadA11yKey) { Remove-Item -Path $a11yKey -ErrorAction SilentlyContinue }
    if ($p -and -not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-48} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Text scaling' $lines $ok
if (-not $ok) { $failed++ }

# Popups (TD-0009 A1) ------------------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class PopupGateWin32 {
    delegate bool EnumProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr hwnd, StringBuilder name, int size);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowLongW(IntPtr hwnd, int index);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetDoubleClickTime();
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public InputUnion u; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);

    // The right button, or the left, pressed and let go where the pointer is, as mouse input.
    // Returns the events sent.
    public static uint Click(bool right) {
        var inputs = new INPUT[2];
        inputs[0].u.mi.dwFlags = right ? 0x0008u : 0x0002u; // MOUSEEVENTF_RIGHTDOWN, LEFTDOWN
        inputs[1].u.mi.dwFlags = right ? 0x0010u : 0x0004u; // MOUSEEVENTF_RIGHTUP, LEFTUP
        return SendInput(2, inputs, Marshal.SizeOf(typeof(INPUT)));
    }

    // The windows of class dismpro.popup that `owner` owns (GW_OWNER), the visible ones only if
    // `shown`.
    public static IntPtr[] Popups(IntPtr owner, bool shown) {
        var found = new List<IntPtr>();
        var name = new StringBuilder(64);
        EnumWindows((hwnd, data) => {
            name.Clear();
            if (GetWindow(hwnd, 4) == owner && (!shown || IsWindowVisible(hwnd))
                && GetClassNameW(hwnd, name, name.Capacity) > 0 && name.ToString() == "dismpro.popup") {
                found.Add(hwnd);
            }
            return true;
        }, IntPtr.Zero);
        return found.ToArray();
    }

    // The window's rectangle on the screen: left, top, right, bottom.
    public static int[] Rect(IntPtr hwnd) {
        RECT r;
        if (!GetWindowRect(hwnd, out r)) { return new int[] { 0, 0, 0, 0 }; }
        return new int[] { r.Left, r.Top, r.Right, r.Bottom };
    }

    public static int ExStyle(IntPtr hwnd) { return GetWindowLongW(hwnd, -20); } // GWL_EXSTYLE
}
'@
$toggleState = [Windows.Automation.ToggleState]
$expandState = [Windows.Automation.ExpandCollapseState]
$tipText = 'Every change before it applies'

function Get-Shown([IntPtr] $Owner) { @([PopupGateWin32]::Popups($Owner, $true)) }
# The shown popup whose tree has a `$Type` named `$Name`: its window and that element, or $null.
function Find-Popup([IntPtr] $Owner, $Type, [string] $Name) {
    foreach ($h in (Get-Shown $Owner)) {
        try { $e = Find-Elements ($ae::FromHandle($h)) $Type $Name | Select-Object -First 1 } catch { $e = $null }
        if ($null -ne $e) { return @{ Hwnd = $h; Element = $e } }
    }
    $null
}
# UIA's focused element is a `$Type` named `$Name` (FocusedElement, not the element's own flag).
function Test-Focused($Type, [string] $Name) {
    try {
        $f = $ae::FocusedElement
        $null -ne $f -and $f.Current.ControlType -eq $Type -and $f.Current.Name -eq $Name
    } catch { $false }
}
function Format-Ms([double] $Ms, [double] $Advisory = 100) {
    '{0:N1} ms{1}' -f $Ms, $(if ($Ms -gt $Advisory) { " (advisory: over $Advisory ms)" } else { '' })
}
$lines = @("Popups on the shell at $Size (the checks are gates, each read back through UIA within 2 s; opening is advisory: <= 100 ms from the key or click to UIA seeing it, U3)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $rows = @()
    $found = Wait-Until {
        $script:rows = @(Get-Rows $root)
        $script:rows.Count -gt 4
    } 10000
    if (-not $found) { throw 'no table rows in 10 s' }
    $row = $rows[3]
    $label = { param([string] $Text) (Find-Elements $root $types::Text $Text).Count -eq 1 }
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)
    $p.Refresh()
    $before = $p.WorkingSet64

    # A row's menu from the keyboard: Shift+F10 on the focused row. UIA's FocusedElement follows
    # the popup's keyboard focus (A1.3's verify).
    $row.SetFocus()
    $focused = Wait-Until { $row.Current.HasKeyboardFocus } 2000
    $menu = $null
    $sw = [Diagnostics.Stopwatch]::StartNew()
    [void][TextGateWin32]::Chord($shift, 0x79) # Shift+F10
    $opened = $focused -and (Wait-Until {
        $script:menu = Find-Popup $hwnd $types::MenuItem 'Uninstall'
        $null -ne $script:menu -and $script:menu.Element.Current.HasKeyboardFocus -and (Test-Focused $types::MenuItem 'Uninstall')
    } 2000)
    $firstMs = $sw.Elapsed.TotalMilliseconds
    $checks['Shift+F10: an owned popup, its first item focused'] = $opened
    if (-not $opened) { throw "Shift+F10 on '$($row.Current.Name)' showed no menu with Uninstall focused in 2 s ($((Get-Shown $hwnd).Count) popups shown, row focused: $focused)" }
    $menuHwnd = $menu.Hwnd
    $lines += "first open:  Shift+F10 to its first item focused in UIA, $(Format-Ms $firstMs)"
    $p.Refresh()
    $lines += '  working set {0:+0.0;-0.0;0.0} MiB with the menu open (advisory: <= 2 MiB, A1.7)' -f (($p.WorkingSet64 - $before) / 1MB)
    $lines += Save-Screen ([PopupGateWin32]::Rect($menuHwnd)) 'popup-menu.png'
    $menuRoot = $ae::FromHandle($menuHwnd)
    $items = @(Find-Elements $menuRoot $types::MenuItem | ForEach-Object { $_.Current.Name })
    $checks['a Menu "Row" of its 5 MenuItems'] = (Find-Elements $menuRoot $types::Menu 'Row').Count -eq 1 -and ($items -join ',') -eq 'Uninstall,Open file location,Copy name,Copy ID,Properties'
    $checks['a tool window, not on the taskbar'] = ([PopupGateWin32]::ExStyle($menuHwnd) -band 0x80) -ne 0 # WS_EX_TOOLWINDOW
    Send-Key 0x28 # Down
    $checks['Down moves the focus to the next item'] = Wait-Until { Test-Focused $types::MenuItem 'Open file location' } 2000
    Send-Key 0x0D # Enter
    $checks['Enter invokes it, closes, focus back on the row'] = Wait-Until {
        (& $label 'Opened file location') -and (Get-Shown $hwnd).Count -eq 0 -and $row.Current.HasKeyboardFocus
    } 2000

    # Again: the same window, shown again; Esc closes it with another item focused.
    $sw.Restart()
    [void][TextGateWin32]::Chord($shift, 0x79)
    $again = Wait-Until {
        $script:menu = Find-Popup $hwnd $types::MenuItem 'Uninstall'
        $null -ne $script:menu -and (Test-Focused $types::MenuItem 'Uninstall')
    } 2000
    $lines += "second open: $(Format-Ms $sw.Elapsed.TotalMilliseconds)"
    $checks['a second Shift+F10 reuses the window'] = $again -and $menu.Hwnd -eq $menuHwnd -and @([PopupGateWin32]::Popups($hwnd, $false)).Count -eq 1
    Send-Key 0x28
    Send-Key 0x28
    $moved = Wait-Until { Test-Focused $types::MenuItem 'Copy name' } 2000
    Send-Key 0x1B # Esc
    $closed = Wait-Until { (Get-Shown $hwnd).Count -eq 0 -and $row.Current.HasKeyboardFocus } 2000
    Start-Sleep -Milliseconds 200 # an invoked item would have shown by now
    $checks['Esc closes it, invoking nothing'] = $again -and $moved -and $closed -and (& $label 'Opened file location')

    # From the pointer: a right-click on the next row picks it and opens its menu at the pointer;
    # a click outside closes it and reaches nothing (the Games check box stays off), and the next
    # click there does.
    $next = $rows[4]
    $r = $next.Current.BoundingRectangle
    $at = @([int] ($r.X + $r.Width / 3), [int] ($r.Y + $r.Height / 2))
    [void][TextGateWin32]::MoveTo($at[0], $at[1])
    $sw.Restart()
    [void][PopupGateWin32]::Click($true)
    $opened = Wait-Until { $script:menu = Find-Popup $hwnd $types::MenuItem 'Uninstall'; $null -ne $script:menu } 2000
    $lines += "right-click open: $(Format-Ms $sw.Elapsed.TotalMilliseconds)"
    $checks['a right-click on a row picks it and opens its menu'] = $opened -and (Wait-Until { Test-Selected $next } 2000)
    if ($opened) {
        $m = [PopupGateWin32]::Rect($menu.Hwnd)
        $lines += "  the menu at ($($m[0]), $($m[1])), the click at ($($at[0]), $($at[1]))"
        $checks['at the pointer, below it'] = [Math]::Abs($m[0] - $at[0]) -le 3 -and $m[1] - $at[1] -ge -3 -and $m[1] - $at[1] -le 12
    }
    $games = Find-First $root $types::CheckBox 'Games'
    $toggle = Get-Pattern $games ([Windows.Automation.TogglePattern]::Pattern)
    $g = $games.Current.BoundingRectangle
    $gx = [int] ($g.X + $g.Width / 2)
    $gy = [int] ($g.Y + $g.Height / 2)
    $inside = $opened -and $gx -ge $m[0] -and $gx -lt $m[2] -and $gy -ge $m[1] -and $gy -lt $m[3]
    if ($inside) { throw "the Games check box at ($gx, $gy) is under the menu: no click outside to make" }
    [void][TextGateWin32]::MoveTo($gx, $gy)
    [void][PopupGateWin32]::Click($false)
    $closed = Wait-Until { (Get-Shown $hwnd).Count -eq 0 } 2000
    Start-Sleep -Milliseconds 200 # a click that reached the box would have toggled it by now
    $checks['a click outside closes it and reaches nothing'] = $opened -and $closed -and $toggle.Current.ToggleState -eq $toggleState::Off
    [void][PopupGateWin32]::Click($false)
    $checks['the next click there reaches the check box'] = Wait-Until { $toggle.Current.ToggleState -eq $toggleState::On } 2000

    # A right-click outside an open menu, on another row above it, closes the menu and asks for
    # no other: its release is eaten with its press. The next right-click there opens that row's.
    [void][TextGateWin32]::MoveTo($at[0], $at[1])
    [void][PopupGateWin32]::Click($true)
    $reopened = Wait-Until { (Get-Shown $hwnd).Count -eq 1 } 2000
    $o = $rows[2].Current.BoundingRectangle
    $ox = [int] ($o.X + $o.Width / 3)
    $oy = [int] ($o.Y + $o.Height / 2)
    if ($reopened) {
        $m = [PopupGateWin32]::Rect((Get-Shown $hwnd)[0])
        if ($ox -ge $m[0] -and $ox -lt $m[2] -and $oy -ge $m[1] -and $oy -lt $m[3]) { throw "row 2 at ($ox, $oy) is under the menu: no right-click outside to make" }
    }
    [void][TextGateWin32]::MoveTo($ox, $oy)
    [void][PopupGateWin32]::Click($true)
    $closed = Wait-Until { (Get-Shown $hwnd).Count -eq 0 } 2000
    Start-Sleep -Milliseconds 300 # a menu its release asked for would show by now
    $checks['a right-click outside closes the menu and opens no other'] = $reopened -and $closed -and (Get-Shown $hwnd).Count -eq 0
    [void][PopupGateWin32]::Click($true)
    $checks['the next right-click there opens that row''s menu'] = (Wait-Until { (Get-Shown $hwnd).Count -eq 1 } 2000) -and (Wait-Until { Test-Selected $rows[2] } 2000)
    Send-Key 0x1B # Esc
    [void](Wait-Until { (Get-Shown $hwnd).Count -eq 0 } 2000)

    # A tooltip: the pointer resting on Review changes shows its tip after Windows' delay (the
    # double-click time); the tip never activates, and goes when the pointer leaves.
    $delay = [int] [PopupGateWin32]::GetDoubleClickTime()
    $b = (Find-First $root $types::Button 'Review changes').Current.BoundingRectangle
    $tip = $null
    [void][TextGateWin32]::MoveTo([int] ($b.X + $b.Width / 2), [int] ($b.Y + $b.Height / 2))
    $sw.Restart()
    $showed = Wait-Until { $script:tip = Find-Popup $hwnd $types::ToolTip $tipText; $null -ne $script:tip } ($delay + 2000)
    $tipMs = $sw.Elapsed.TotalMilliseconds
    $lines += "tooltip: shown $(Format-Ms $tipMs ($delay + 100)) after the pointer came to rest (the delay: $delay ms)"
    $checks['hovering shows a ToolTip, not before the delay'] = $showed -and $tipMs -ge $delay - 20
    if ($showed) {
        $lines += Save-Screen ([PopupGateWin32]::Rect($tip.Hwnd)) 'popup-tooltip.png'
        $checks['the tip never activates'] = [PopupGateWin32]::GetForegroundWindow() -eq $hwnd -and ([PopupGateWin32]::ExStyle($tip.Hwnd) -band 0x08000000) -ne 0 # WS_EX_NOACTIVATE
    }
    $t = (Find-First $root $types::Text 'DISM Pro').Current.BoundingRectangle
    [void][TextGateWin32]::MoveTo([int] ($t.X + $t.Width / 2), [int] ($t.Y + $t.Height / 2))
    $checks['the pointer leaving hides it'] = $showed -and (Wait-Until { $null -eq (Find-Popup $hwnd $types::ToolTip $tipText) } 2000)

    # Settings' Start page dropdown: Expand opens its list on the picked option; Down and Enter
    # pick the next; Select in the list picks another; Collapse closes it.
    Select-Item (Find-First $root $types::ListItem 'Settings')
    $combo = $null
    $found = Wait-Until { $script:combo = Find-First $root $types::ComboBox 'Start page'; $null -ne $script:combo } 2000
    if (-not $found) { throw 'no ComboBox "Start page" on Settings in 2 s' }
    $expand = Get-Pattern $combo ([Windows.Automation.ExpandCollapsePattern]::Pattern)
    $value = Get-Pattern $combo ([Windows.Automation.ValuePattern]::Pattern)
    $checks['a ComboBox "Start page", collapsed, at Overview'] = $expand.Current.ExpandCollapseState -eq $expandState::Collapsed -and $value.Current.Value -eq 'Overview'
    [void][GlassGateWin32]::Activate($hwnd)
    $list = $null
    $sw.Restart()
    $expand.Expand()
    $opened = Wait-Until {
        $script:list = Find-Popup $hwnd $types::ListItem 'Overview'
        $null -ne $script:list -and $script:list.Element.Current.HasKeyboardFocus
    } 2000
    $lines += "dropdown open: Expand to the picked option focused, $(Format-Ms $sw.Elapsed.TotalMilliseconds)"
    $checks['Expand opens the list, on the picked option'] = $opened -and (Test-Selected $list.Element) -and $expand.Current.ExpandCollapseState -eq $expandState::Expanded -and (Find-Elements ($ae::FromHandle($list.Hwnd)) $types::List 'Start page').Count -eq 1
    if ($opened) {
        $lines += Save-Screen ([PopupGateWin32]::Rect($list.Hwnd)) 'popup-list.png'
        Send-Key 0x28 # Down
        Send-Key 0x0D # Enter
    }
    $checks['Down, Enter pick the next and close the list'] = $opened -and (Wait-Until {
        $value.Current.Value -eq 'Clean' -and $expand.Current.ExpandCollapseState -eq $expandState::Collapsed -and (Get-Shown $hwnd).Count -eq 0
    } 2000)
    $expand.Expand()
    $repair = $null
    $reopened = Wait-Until { $script:repair = Find-Popup $hwnd $types::ListItem 'Repair'; $null -ne $script:repair } 2000
    if ($reopened) { Select-Item $repair.Element }
    $checks['Select(Repair) in the list picks it and closes'] = $reopened -and (Wait-Until { $value.Current.Value -eq 'Repair' -and (Get-Shown $hwnd).Count -eq 0 } 2000)
    $expand.Expand()
    $reopened = Wait-Until { $null -ne (Find-Popup $hwnd $types::List 'Start page') } 2000
    $expand.Collapse()
    $checks['Collapse closes it, the value kept'] = $reopened -and (Wait-Until {
        (Get-Shown $hwnd).Count -eq 0 -and $expand.Current.ExpandCollapseState -eq $expandState::Collapsed
    } 2000) -and $value.Current.Value -eq 'Repair'
} catch {
    $lines += "popups FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Popups' $lines $ok
if (-not $ok) { $failed++ }

# Scrolling (TD-0009 A2, slices 5a and 5b1) ---------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ScrollGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public InputUnion u; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    static extern bool SystemParametersInfoW(uint action, uint param, ref uint value, uint ini);
    // One wheel event of `delta` (120 a notch; positive is away from the user, or right):
    // MOUSEEVENTF_WHEEL, or MOUSEEVENTF_HWHEEL. Returns the events sent.
    public static uint Wheel(int delta, bool horizontal) {
        var i = new INPUT { type = 0 };
        i.u.mi = new MOUSEINPUT { mouseData = unchecked((uint) delta), dwFlags = horizontal ? 0x1000u : 0x0800u };
        return SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT)));
    }
    // SPI_GETWHEELSCROLLLINES; 0 if it couldn't be read.
    public static uint WheelLines() {
        uint v = 0;
        return SystemParametersInfoW(0x0068, 0, ref v, 0) ? v : 0;
    }
}
'@
function Send-Chord([uint16] $Mod, [uint16] $Vk) {
    [void][ControlsGateWin32]::Key($Mod, $true)
    Send-Key $Vk
    [void][ControlsGateWin32]::Key($Mod, $false)
}
# Whether `$Inner`'s rectangle lies inside `$Outer`'s, to half a pixel.
function Test-Inside($Inner, $Outer) {
    $Inner.Y -ge $Outer.Y - 0.5 -and $Inner.Bottom -le $Outer.Bottom + 0.5
}

$lines = @("Scrolling in the shell at $Size (the checks are gates; the times are advisory, U3: 50 ms)")

# "Automatically hide scroll bars" (A2.3's verify, advisory): no Microsoft page says the value
# behind it is DynamicScrollbars, which the shell's Env reads. UISettings is WinRT, read through
# Windows PowerShell 5.1 as for the text size.
function Get-UISettingsAutoHide {
    $read = '[void][Windows.UI.ViewManagement.UISettings,Windows.UI.ViewManagement,ContentType=WindowsRuntime]; (New-Object Windows.UI.ViewManagement.UISettings).AutoHideScrollBars'
    try { (& powershell.exe -NoProfile -NonInteractive -Command $read 2>&1 | Out-String).Trim() } catch { "unreadable: $($_.Exception.Message)" }
}
$cpaKey = 'HKCU:\Control Panel\Accessibility'
$hadCpaKey = Test-Path $cpaKey
$savedDynamic = if ($hadCpaKey) { (Get-ItemProperty -Path $cpaKey -Name DynamicScrollbars -ErrorAction SilentlyContinue).DynamicScrollbars } else { $null }
# Env takes only a REG_DWORD 0 as "show them", so the kind is reported and kept.
$savedKind = if ($null -ne $savedDynamic) { (Get-Item -Path $cpaKey).GetValueKind('DynamicScrollbars') } else { $null }
$lines += 'auto-hide scroll bars: DynamicScrollbars {0}, UISettings.AutoHideScrollBars {1}' -f $(if ($null -eq $savedDynamic) { 'missing' } else { "$savedDynamic ($savedKind)" }), (Get-UISettingsAutoHide)
try {
    if (-not $hadCpaKey) { $null = New-Item -Path $cpaKey -Force }
    foreach ($v in 0, 1) {
        Set-ItemProperty -Path $cpaKey -Name DynamicScrollbars -Value $v -Type DWord
        $lines += '  DynamicScrollbars written {0}: UISettings.AutoHideScrollBars {1}' -f $v, (Get-UISettingsAutoHide)
    }
} catch {
    $lines += "  writing DynamicScrollbars FAILED: $($_.Exception.Message)"
} finally {
    if ($null -ne $savedDynamic) {
        Set-ItemProperty -Path $cpaKey -Name DynamicScrollbars -Value $savedDynamic -Type $savedKind
    } elseif ($hadCpaKey) {
        Remove-ItemProperty -Path $cpaKey -Name DynamicScrollbars -ErrorAction SilentlyContinue
    } else {
        Remove-Item -Path $cpaKey -ErrorAction SilentlyContinue
    }
}

$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $table = $null
    $found = Wait-Until { $script:table = Find-First $root $types::DataGrid 'Programs'; $null -ne $script:table } 10000
    if (-not $found) { throw 'no DataGrid "Programs" in 10 s' }
    $rows = @(Get-Rows $root)
    $view = $table.Current.BoundingRectangle
    $first = $rows[0]
    $firstName = $first.Current.Name
    $last = $rows[$rows.Count - 1]
    $y0 = $first.Current.BoundingRectangle.Y
    # UIA sees every row built, half a view past the end too (see U12).
    $checks['the last row UIA sees is out of view'] = $rows.Count -gt 1 -and -not (Test-Inside $last.Current.BoundingRectangle $view)
    $lines += 'table {0:N0}x{1:N0} at {2:N0},{3:N0}; {4} rows built; Windows scrolls {5} lines a notch' -f $view.Width, $view.Height, $view.X, $view.Y, $rows.Count, [ScrollGateWin32]::WheelLines()
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)

    # The wheel, through SendInput, at the table's center.
    [void][ScrollGateWin32]::SetCursorPos([int] ($view.X + $view.Width / 2), [int] ($view.Y + $view.Height / 2))
    $sw = [Diagnostics.Stopwatch]::StartNew()
    [void][ScrollGateWin32]::Wheel(-120, $false)
    $moved = Wait-Until { $first.Current.BoundingRectangle.Y -lt $y0 } 2000
    $ms = $sw.Elapsed.TotalMilliseconds
    $checks['a notch down moves the rows up'] = $moved
    $lines += '{0,-12} {1,7:N1} ms  rows moved {2:N0} px' -f 'notch', $ms, ($y0 - $first.Current.BoundingRectangle.Y)
    [void][ScrollGateWin32]::Wheel(120, $false)
    $checks['a notch up moves them back'] = Wait-Until { $first.Current.BoundingRectangle.Y -eq $y0 } 2000

    # On the focused row, Ctrl+End makes the table's last row current: it's built, focused and in
    # view. Ctrl+Home goes back to the first, built again.
    $first.SetFocus()
    $focused = Wait-Until { $first.Current.HasKeyboardFocus } 2000
    $sw.Restart()
    Send-Chord 0x11 0x23
    $end = $focused -and (Wait-Until {
        $f = $ae::FocusedElement
        $f.Current.Name -eq 'Northwind Toolbox 998' -and (Test-Inside $f.Current.BoundingRectangle $view)
    } 2000)
    $ms = $sw.Elapsed.TotalMilliseconds
    $checks['Ctrl+End focuses the last row, in view'] = $end
    $lines += '{0,-12} {1,7:N1} ms' -f 'Ctrl+End', $ms
    Send-Chord 0x11 0x24
    $checks['Ctrl+Home shows the first again'] = Wait-Until {
        $top = @(Get-Rows $root) | Select-Object -First 1
        $null -ne $top -and $top.Current.Name -eq $firstName -and $top.Current.BoundingRectangle.Y -eq $y0
    } 2000

    # Focus moving to a row out of view brings it into view, and makes it current.
    $last = @(Get-Rows $root) | Select-Object -Last 1
    $last.SetFocus()
    $checks['SetFocus on the last row UIA sees scrolls it into view'] = Wait-Until { $last.Current.HasKeyboardFocus -and (Test-Inside $last.Current.BoundingRectangle $view) } 2000
} catch {
    $lines += "scrolling FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Scrolling' $lines $ok
if (-not $ok) { $failed++ }

# Columns (TD-0009 A2, slice 5b2) ------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ColumnsGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public InputUnion u; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
    // The left button pressed, or let go, where the pointer is, as mouse input. Returns the
    // events sent.
    public static uint Button(bool down) {
        var inputs = new INPUT[1];
        inputs[0].u.mi.dwFlags = down ? 0x0002u : 0x0004u; // MOUSEEVENTF_LEFTDOWN, LEFTUP
        return SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@
# The HeaderItem named `$Name` in the shell's tree, or $null.
function Get-Header($Root, [string] $Name) { Find-First $Root $types::HeaderItem $Name }
# The middle of `$Header`'s right edge, where its divider is, in screen pixels.
function Get-Divider($Header) {
    $r = $Header.Current.BoundingRectangle
    @([int] [Math]::Round($r.Right), [int] [Math]::Round($r.Y + $r.Height / 2))
}

$lines = @("Columns in the shell at $Size (the checks are gates, each read back through UIA within 2 s; the drag's time is advisory, U3: 50 ms)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $publisher = $null
    $found = Wait-Until { $script:publisher = Get-Header $root 'Publisher'; $null -ne $script:publisher } 10000
    if (-not $found) { throw 'no HeaderItem "Publisher" in 10 s' }
    $name = Get-Header $root 'Name'
    $nameW = $name.Current.BoundingRectangle.Width
    $w0 = $publisher.Current.BoundingRectangle.Width
    $lines += 'Name {0:N0} px, Publisher {1:N0} px wide' -f $nameW, $w0
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)

    # The divider: the left-right arrows over it, then a drag through SendInput.
    $at = Get-Divider $publisher
    [void][TextGateWin32]::MoveTo($at[0] - 1, $at[1])
    $hit = $false
    $pointer = ''
    $checks['the pointer on a divider shows the left-right arrows'] = Wait-Until { $script:pointer = [TextGateWin32]::Pointer($hwnd, 32644, [ref] $script:hit); $script:hit } 2000
    $lines += "pointer on Publisher's divider: $pointer"
    [void][ColumnsGateWin32]::Button($true)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    foreach ($dx in 20, 40, 60) { [void][TextGateWin32]::MoveTo($at[0] - 1 + $dx, $at[1]) }
    $live = Wait-Until { [Math]::Abs($publisher.Current.BoundingRectangle.Width - ($w0 + 60)) -le 1 } 2000
    $lines += '{0,-12} {1}' -f 'drag', (Format-Ms $sw.Elapsed.TotalMilliseconds 50)
    [void][ColumnsGateWin32]::Button($false)
    Start-Sleep -Milliseconds 200 # a release that put the column back would have by now
    $w1 = $publisher.Current.BoundingRectangle.Width
    $lines += '  Publisher {0:N0} px after the drag, Name {1:N0} px' -f $w1, $name.Current.BoundingRectangle.Width
    $checks['a drag 60 px right widens the column 60 px'] = $live -and [Math]::Abs($w1 - ($w0 + 60)) -le 1
    $checks['the column left of it holds still'] = [Math]::Abs($name.Current.BoundingRectangle.Width - $nameW) -le 0.5

    # A double-click on the divider fits the column to its header and the cells built.
    $at = Get-Divider $publisher
    [void][TextGateWin32]::MoveTo($at[0] - 1, $at[1])
    [void][PopupGateWin32]::Click($false)
    [void][PopupGateWin32]::Click($false)
    $fitted = Wait-Until { [Math]::Abs($publisher.Current.BoundingRectangle.Width - $w1) -gt 1 } 2000
    Start-Sleep -Milliseconds 200
    $w2 = $publisher.Current.BoundingRectangle.Width
    Start-Sleep -Milliseconds 200
    $lines += '  Publisher {0:N0} px fitted' -f $w2
    $checks['a double-click on the divider fits the column'] = $fitted -and $publisher.Current.BoundingRectangle.Width -eq $w2

    # The column menu from the keyboard, on the focused header.
    $publisher.SetFocus()
    $focused = Wait-Until { $publisher.Current.HasKeyboardFocus } 2000
    [void][TextGateWin32]::Chord($shift, 0x79) # Shift+F10
    $menu = $null
    $opened = $focused -and (Wait-Until { $script:menu = Find-Popup $hwnd $types::Menu 'Columns'; $null -ne $script:menu } 2000)
    if (-not $opened) { throw "Shift+F10 on the Publisher header showed no Menu ""Columns"" in 2 s (header focused: $focused, $((Get-Shown $hwnd).Count) popups shown)" }
    $menuRoot = $ae::FromHandle($menu.Hwnd)
    $items = @(Find-Elements $menuRoot $types::MenuItem | ForEach-Object { $_.Current.Name })
    $lines += "  the menu: $($items -join ', ')"
    $checks['a Menu "Columns": the hideable columns, then its commands'] = ($items -join ',') -eq 'Publisher,Version,Size,Installed,Status,Location,ID,Size column to fit,Size all columns to fit,Reset columns'
    $item = Find-First $menuRoot $types::MenuItem 'Publisher'
    $toggle = Get-Pattern $item ([Windows.Automation.TogglePattern]::Pattern)
    $checks['a shown column is a toggled MenuItem'] = $toggle.Current.ToggleState -eq $toggleState::On
    $toggle.Toggle()
    $checks['Toggle hides the column and closes the menu'] = Wait-Until { (Get-Shown $hwnd).Count -eq 0 -and $null -eq (Get-Header $root 'Publisher') } 2000
    $checks['focus moves to the next header'] = Wait-Until { Test-Focused $types::HeaderItem 'Version' } 2000

    [void][TextGateWin32]::Chord($shift, 0x79)
    $opened = Wait-Until { $script:menu = Find-Popup $hwnd $types::MenuItem 'Reset columns'; $null -ne $script:menu } 2000
    if ($opened) { (Get-Pattern $menu.Element ([Windows.Automation.InvokePattern]::Pattern)).Invoke() }
    $back = $null
    $checks['Reset columns brings it back at its first width'] = $opened -and (Wait-Until {
        $script:back = Get-Header $root 'Publisher'
        $null -ne $script:back -and [Math]::Abs($script:back.Current.BoundingRectangle.Width - $w0) -le 0.5
    } 2000)
} catch {
    $lines += "columns FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Columns' $lines $ok
if (-not $ok) { $failed++ }

# Value cells (TD-0009 A2.5, slice 5b3) -------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class ValuesGateWin32 {
    [StructLayout(LayoutKind.Sequential)]
    struct SYSTEMTIME { public ushort Year, Month, DayOfWeek, Day, Hour, Minute, Second, Milliseconds; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern int GetLocaleInfoEx(string locale, uint type, StringBuilder data, int size);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern int GetDateFormatEx(string locale, uint flags, ref SYSTEMTIME date, string format, StringBuilder data, int size, string calendar);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool SetLocaleInfoW(uint lcid, uint type, string data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, UIntPtr wp, string lp, uint flags, uint ms, out UIntPtr result);
    public const uint Decimal = 0x000E;   // LOCALE_SDECIMAL
    public const uint ShortDate = 0x001F; // LOCALE_SSHORTDATE
    // The user's value of `type`, as the shell reads it.
    public static string Get(uint type) {
        var b = new StringBuilder(128);
        return GetLocaleInfoEx(null, type, b, b.Capacity) > 0 ? b.ToString() : null;
    }
    // The user's short date of a day, as the shell writes it.
    public static string Date(int y, int m, int d) {
        var t = new SYSTEMTIME { Year = (ushort) y, Month = (ushort) m, Day = (ushort) d };
        var b = new StringBuilder(128);
        return GetDateFormatEx(null, 1 /* DATE_SHORTDATE */, ref t, null, b, b.Capacity, null) > 0 ? b.ToString() : null;
    }
    // Sets the user's `type` (LOCALE_USER_DEFAULT) and tells every window, as Settings does.
    public static bool Set(uint type, string value) {
        if (!SetLocaleInfoW(0x0400, type, value)) { return false; }
        UIntPtr r;
        // HWND_BROADCAST, WM_SETTINGCHANGE "intl", SMTO_ABORTIFHUNG, 5 s.
        return SendMessageTimeoutW(new IntPtr(0xFFFF), 0x001A, UIntPtr.Zero, "intl", 0x0002, 5000, out r) != IntPtr.Zero;
    }
}
'@
# The names of the cells of the row named `$Name`, or @() if it isn't built.
function Get-CellNames($Root, [string] $Name) {
    $row = Get-Rows $Root | Where-Object { $_.Current.Name -eq $Name } | Select-Object -First 1
    if ($null -eq $row) { return , @() }
    , @($row.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition) | ForEach-Object { $_.Current.Name })
}

# Row 1 of the shell's data: 37 MiB and 7,919 bytes, installed on 2026-01-02.
$valueRow = 'Fabrikam Updater Pro 1'
$lines = @("Value cells in the shell at $Size (the checks are gates, each read back through UIA within 2 s; the times are advisory, U3: 50 ms)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$oldDecimal = [ValuesGateWin32]::Get([ValuesGateWin32]::Decimal)
$oldDate = [ValuesGateWin32]::Get([ValuesGateWin32]::ShortDate)
$changed = $false
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $sizeText = "37${oldDecimal}0 MB"
    $day = [ValuesGateWin32]::Date(2026, 1, 2)
    $lines += "the runner's formats: decimal '$oldDecimal', short date '$oldDate' ($day)"
    $names = @()
    $read = Wait-Until { $script:names = Get-CellNames $root $valueRow; $script:names.Count -gt 0 } 10000
    if (-not $read) { throw "no row ""$valueRow"" in 10 s" }
    $lines += "  $valueRow`: $($names -join ' | ')"
    $checks['a size reads in the user''s decimal separator'] = $names -contains $sizeText
    $checks['a day reads in the user''s short date'] = $names -contains $day

    # New formats, as Settings › Time & language › Region writes them.
    $decimal = if ($oldDecimal -eq ',') { '.' } else { ',' }
    $changed = $true
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $set = [ValuesGateWin32]::Set([ValuesGateWin32]::Decimal, $decimal) -and [ValuesGateWin32]::Set([ValuesGateWin32]::ShortDate, 'yyyy-MM-dd')
    if (-not $set) { throw "SetLocaleInfoW or the broadcast failed ($([Runtime.InteropServices.Marshal]::GetLastWin32Error()))" }
    $followed = Wait-Until { $n = Get-CellNames $root $valueRow; ($n -contains "37${decimal}0 MB") -and ($n -contains '2026-01-02') } 2000
    $lines += '{0,-12} {1}' -f 'new formats', (Format-Ms $sw.Elapsed.TotalMilliseconds 50)
    $lines += "  $valueRow`: $((Get-CellNames $root $valueRow) -join ' | ')"
    $checks['new formats show in the cells (WM_SETTINGCHANGE "intl")'] = $followed

    $restored = [ValuesGateWin32]::Set([ValuesGateWin32]::Decimal, $oldDecimal) -and [ValuesGateWin32]::Set([ValuesGateWin32]::ShortDate, $oldDate)
    $changed = -not $restored
    $checks['the old formats come back'] = $restored -and (Wait-Until { $n = Get-CellNames $root $valueRow; ($n -contains $sizeText) -and ($n -contains $day) } 2000)
} catch {
    $lines += "value cells FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if ($changed) {
        [void][ValuesGateWin32]::Set([ValuesGateWin32]::Decimal, $oldDecimal)
        [void][ValuesGateWin32]::Set([ValuesGateWin32]::ShortDate, $oldDate)
    }
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Value cells' $lines $ok
if (-not $ok) { $failed++ }


# Data table (TD-0009 A2.5, A2.9, slice 5c3) ---------------------------------------------------------
# The shell with 100,000 data rows: 75,000 on its first tab, which leaves every fourth (a Store app)
# to the other. Data row i is "{vendor i % 4} {product (i / 4) % 4} {i}".
$dataRows = 75000
# Data row 99,998, the view's last.
$lastName = 'Northwind Backup Agent 99998'
# Data row 50,000, the view's 37,501st: far from both ends, so never built before it's found.
$midName = 'Contoso Updater Pro 50000'
# A pattern's call that throws (an element gone, a row UIA couldn't realize) reads as $null.
function Get-Or-Null([scriptblock] $Call) { try { & $Call } catch { $null } }
function Get-Toggle($Element) { (Get-Pattern $Element ([Windows.Automation.TogglePattern]::Pattern)).Current.ToggleState }

$lines = @("Data table: the shell with 100,000 data rows at $Size, $dataRows on its first tab, through UIA's patterns (the checks are gates, each read back within 2 s; the times are advisory, U3: 50 ms)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size, '--rows', '100000') -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $table = $null
    $found = Wait-Until { $script:table = Find-First $root $types::DataGrid 'Programs'; $null -ne $script:table } 10000
    if (-not $found) { throw 'no DataGrid "Programs" in 10 s' }
    $view = $table.Current.BoundingRectangle
    $grid = Get-Pattern $table ([Windows.Automation.GridPattern]::Pattern)
    $built = @(Get-Rows $root).Count
    $lines += 'grid: {0:N0} rows, {1} columns; {2} rows built' -f $grid.Current.RowCount, $grid.Current.ColumnCount, $built
    $checks['GridPattern counts every row'] = $grid.Current.RowCount -eq $dataRows
    $checks['far fewer rows are built (<= 100)'] = $built -gt 0 -and $built -le 100

    # The last row's Publisher cell: not built, so the adapter asks the shell to build it.
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $cell = $grid.GetItem($dataRows - 1, 2)
    $lines += '{0,-12} {1}' -f 'GetItem', (Format-Ms $sw.Elapsed.TotalMilliseconds 50)
    $at = Get-Pattern $cell ([Windows.Automation.GridItemPattern]::Pattern)
    $checks['GetItem(last, 2) gives that cell'] = $at.Current.Row -eq ($dataRows - 1) -and $at.Current.Column -eq 2
    $checks['the view scrolls to it'] = Wait-Until { Test-Inside $cell.Current.BoundingRectangle $view } 2000
    $heads = Get-Pattern $cell ([Windows.Automation.TableItemPattern]::Pattern)
    $column = @($heads.Current.GetColumnHeaderItems() | ForEach-Object { $_.Current.Name })
    $row = @($heads.Current.GetRowHeaderItems() | ForEach-Object { $_.Current.Name })
    $lines += "  its headers: column '$($column -join ', ')', row '$($row -join ', ')'"
    $checks['TableItem gives its column''s header and its row''s'] = ($column -join '|') -eq 'Publisher' -and ($row -join '|') -eq $lastName

    # A row far up, by name: the shell searches its view and builds the row it finds.
    $container = Get-Pattern $table ([Windows.Automation.ItemContainerPattern]::Pattern)
    $sw.Restart()
    $item = $container.FindItemByProperty($null, $ae::NameProperty, $midName)
    $lines += '{0,-12} {1}' -f 'find by name', (Format-Ms $sw.Elapsed.TotalMilliseconds 50)
    $checks['ItemContainer finds a far row by Name'] = $null -ne $item -and $item.Current.Name -eq $midName -and $item.Current.ControlType -eq $types::DataItem
    $checks['the row found is in view'] = $null -ne $item -and (Wait-Until { Test-Inside $item.Current.BoundingRectangle $view } 2000)
    # Started after the row found (an element the client got from this table), the search finds
    # nothing: $null, not an error.
    $again = 'no first row'
    if ($null -ne $item) {
        $again = try { $container.FindItemByProperty($item, $ae::NameProperty, $midName) } catch { "an error: $($_.Exception.Message)" }
    }
    if ($null -ne $again) { $lines += "  after it: $(if ($again -is [string]) { $again } else { $again.Current.Name })" }
    $checks['after it, no other row has that name'] = $null -eq $again

    # Scrolled to the end through ScrollPattern, then a wheel notch back up through SendInput.
    $scroll = Get-Pattern $table ([Windows.Automation.ScrollPattern]::Pattern)
    $scroll.SetScrollPercent([Windows.Automation.ScrollPattern]::NoScroll, 100)
    $lastRow = $null
    $checks['SetScrollPercent(-1, 100) shows the last row'] = Wait-Until {
        $script:lastRow = Get-Rows $root | Where-Object { $_.Current.Name -eq $lastName } | Select-Object -First 1
        $null -ne $script:lastRow -and (Test-Inside $script:lastRow.Current.BoundingRectangle $view)
    } 2000
    $top = (Get-Rows $root | Select-Object -First 1).Current.Name
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)
    [void][ScrollGateWin32]::SetCursorPos([int] ($view.X + $view.Width / 2), [int] ($view.Y + $view.Height / 2))
    [void][ScrollGateWin32]::Wheel(120, $false)
    $checks['a wheel notch changes the first row built'] = Wait-Until { (Get-Rows $root | Select-Object -First 1).Current.Name -ne $top } 2000

    # The last row's check cell, toggled through UIA: the row is checked, and the check column's
    # header box follows (the probe's toolbar shows the current row, not a count). Then unchecked.
    $tableHeads = @((Get-Pattern $table ([Windows.Automation.TablePattern]::Pattern)).Current.GetColumnHeaders())
    $box = $tableHeads[0]
    $check = Get-Or-Null { $grid.GetItem($dataRows - 1, 0) }
    if ($null -eq $check) { throw 'no check cell for the last row' }
    (Get-Pattern $check ([Windows.Automation.TogglePattern]::Pattern)).Toggle()
    $on = [Windows.Automation.ToggleState]::On
    $mixed = [Windows.Automation.ToggleState]::Indeterminate
    $off = [Windows.Automation.ToggleState]::Off
    $checks['Toggle on a check cell checks the row'] = Wait-Until { (Get-Toggle $check) -eq $on } 2000
    $checks['the header''s box follows (Mixed)'] = Wait-Until { (Get-Toggle $box) -eq $mixed } 2000
    (Get-Pattern $check ([Windows.Automation.TogglePattern]::Pattern)).Toggle()
    $checks['Toggle again unchecks it, and the box'] = Wait-Until { ((Get-Toggle $check) -eq $off) -and ((Get-Toggle $box) -eq $off) } 2000

    # The current row is the selected one.
    $lastRow = Get-Rows $root | Where-Object { $_.Current.Name -eq $lastName } | Select-Object -First 1
    if ($null -eq $lastRow) { throw 'the last row is no longer built' }
    $lastRow.SetFocus()
    $checks['the current row''s SelectionItem is selected'] = Wait-Until { $lastRow.Current.HasKeyboardFocus -and (Test-Selected $lastRow) } 2000

    # TablePattern's headers are the header's items, the check box's first; Invoke on Name's sorts
    # by it (the data's order), and again reverses it, so the first row is the last one.
    $headNames = @($tableHeads | ForEach-Object { $_.Current.Name })
    $itemNames = @(Find-Elements $table $types::HeaderItem $null | ForEach-Object { $_.Current.Name })
    $lines += "  column headers: $($headNames -join ' | ')"
    $checks['TablePattern''s column headers are the header''s'] = $headNames.Count -eq $grid.Current.ColumnCount -and ($headNames -join '|') -eq ($itemNames -join '|')
    $name = $tableHeads | Where-Object { $_.Current.Name -eq 'Name' } | Select-Object -First 1
    if ($null -eq $name) { throw 'no column header "Name"' }
    $invoke = Get-Pattern $name ([Windows.Automation.InvokePattern]::Pattern)
    $invoke.Invoke()
    $invoke.Invoke()
    $checks['Invoke on Name''s header twice reverses the rows'] = Wait-Until { (Get-Or-Null { $grid.GetItem(0, 1).Current.Name }) -eq $lastName } 2000
} catch {
    $lines += "data table FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Data table' $lines $ok
if (-not $ok) { $failed++ }

# Composites: drawer (TD-0009 A3.5 (6), A3.9, slice 6d) ---------------------------------------------
# G1's Drawer check, on the drawer probe (crates/dpro-ui/examples/drawer.rs, fixtures/drawer.rs):
# Enter on "Review changes" opens the drawer, the Group "Preview" (a region landmark), and focus
# moves to its first change; Esc closes it, and focus goes back to "Review changes". Those are gates.
# The slide's frames are counted in a second run under --ui-probe idle, with the same Enter and Esc
# sent inside its 5 s watch (3 s after the first frame): ceil(180 / 16) = 12 frame timers with
# animations on (A3.7's U16), none with them off. The count is advisory, as the runner's timing is.
$drawerExe = Join-Path $Bin 'drawer.exe'
# Waits for the drawer probe `$Process`'s window and returns it with its UIA element.
function Wait-DrawerWindow($Process) {
    $found = [IntPtr]::Zero
    $shown = Wait-Until {
        $script:drawerHwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro drawer probe')
        $script:drawerHwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:drawerHwnd)
    } 30000
    if (-not $shown) { throw "the drawer probe showed no window in 30 s ($(if ($Process.HasExited) { "exited with code $($Process.ExitCode)" } else { 'still running' }))" }
    $found = $script:drawerHwnd
    @{ Hwnd = $found; Root = $ae::FromHandle($found) }
}
# Focuses "Review changes" in `$Window`, active, and returns it.
function Focus-Opener($Window) {
    $opener = $null
    $found = Wait-Until { $script:drawerOpener = Find-First $Window.Root $types::Button 'Review changes'; $null -ne $script:drawerOpener } 10000
    if (-not $found) { throw 'no Button "Review changes" in 10 s' }
    $opener = $script:drawerOpener
    [void][GlassGateWin32]::Activate($Window.Hwnd)
    $opener.SetFocus()
    if (-not (Wait-Until { $opener.Current.HasKeyboardFocus } 2000)) { throw '"Review changes" took no focus in 2 s' }
    $opener
}

$lines = @("Drawer on the drawer probe at $Size (the checks are gates; the slide's frame count is advisory)")
$checks = [ordered]@{}
$p = $null
$window = $null
try {
    if (-not (Test-Path $drawerExe)) { throw "missing $drawerExe (docker compose run --rm t3-tools builds it)" }
    $p = Start-Process -FilePath $drawerExe -ArgumentList @('--size', $Size) -PassThru
    $null = $p.Handle
    $window = Wait-DrawerWindow $p
    $null = Focus-Opener $window
    Send-Key 0x0D
    $drawer = $null
    $checks['Enter on Review changes opens the Group "Preview"'] = Wait-Until { $script:drawer = Find-First $window.Root $types::Group 'Preview'; $null -ne $script:drawer } 2000
    if ($null -eq $drawer) { throw 'no Group "Preview" in 2 s' }
    $first = Find-First $drawer $types::CheckBox 'Temporary files'
    $checks['focus moves to its first change'] = ($null -ne $first) -and (Wait-Until { $first.Current.HasKeyboardFocus } 2000)
    Send-Key 0x1B
    $checks['Esc closes it'] = Wait-Until { (Find-Elements $window.Root $types::Group 'Preview').Count -eq 0 } 2000
    $checks['focus goes back to Review changes'] = Wait-Until {
        $back = Find-First $window.Root $types::Button 'Review changes'
        $null -ne $back -and $back.Current.HasKeyboardFocus
    } 2000
} catch {
    $lines += "drawer FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if ($null -ne $p -and -not $p.HasExited) {
        if ($null -ne $window) { [void][UiaGateWin32]::PostMessageW($window.Hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the drawer probe did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
# The slide's frames (advisory): the idle probe's frame timers, with the drawer opened and closed
# inside its watch.
try {
    if (Test-Path $drawerExe) {
        $report = Join-Path $Out 'drawer-idle.txt'
        Remove-Item $report -ErrorAction SilentlyContinue
        $q = Start-Process -FilePath $drawerExe -ArgumentList @('--size', $Size, '--ui-probe', 'idle', '--probe-out', $report) -PassThru
        $null = $q.Handle
        $idleWindow = Wait-DrawerWindow $q
        Start-Sleep -Milliseconds 3500
        $null = Focus-Opener $idleWindow
        Send-Key 0x0D
        Start-Sleep -Milliseconds 1000
        Send-Key 0x1B
        if (-not $q.WaitForExit(15000)) { $q.Kill(); throw 'the idle probe reported nothing in 15 s' }
        $fields = @{}
        foreach ($pair in ((Get-Content $report -Raw).Trim() -split '\s+')) {
            $key, $value = $pair -split '=', 2
            $fields[$key] = $value
        }
        $timers = [int] $fields.timers
        $lines += 'slide: {0} frame timers and {1} paints in the watch, for an open and a close (12 timers with animations on, 0 off){2}' -f $timers, $fields.paints, $(if ($timers -ne 12 -and $timers -ne 0) { ' (advisory: not 12)' } else { '' })
    }
} catch {
    $lines += "slide count (advisory) failed: $($_.Exception.Message)"
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Composites: drawer' $lines $ok
if (-not $ok) { $failed++ }

# Composites: picker dialog (TD-0009 A3.5 (12), A3.9, slice 6e) ------------------------------------
# G1's picker dialog check, on the file dialog probe (crates/dpro-win/examples/file_dialog_t3.rs),
# once choosing a folder and once a file (two file types), starting in %TEMP%. The probe's window
# asks for the system dialog as the GUI does (WindowCx::pick, shown once its handler returned).
# Gates: a Window named by the probe's title is found through UIA, owned by the probe's window
# (GW_OWNER), which it disables while up (modal); the probe's own tree still answers through UIA
# while it's up (its Button reads "pick: asked": AccessKit's provider while the UI thread is in
# the dialog's single-threaded apartment, A3.5 (12)'s verify); Invoke on its Cancel button
# (AutomationId 2, IDCANCEL, else the Button "Cancel") closes it; the probe reports
# picked=cancelled (the Ok(None) mapping), and ticks >= 1, the timer wakes its window heard through
# the dialog's modal loop; afterwards its window is enabled again and its Button reads
# "pick: cancelled" through UIA (the tree served after the apartment was left). The time from
# Invoke to the report is advisory (100 ms). A screenshot of each dialog goes to -Out.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class FileDialogGateWin32 {
    [DllImport("user32.dll")]
    public static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);
    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr hwnd);
}
'@
$fileDialogExe = Join-Path $Bin 'file_dialog_t3.exe'
$fileDialogProbe = 'DISM Pro file dialog probe'
# Start-Process joins its arguments with spaces and quotes none.
function Format-Arg([string] $Value) { if ($Value -match '\s') { "`"$Value`"" } else { $Value } }

$lines = @("Picker dialog on the file dialog probe, choosing a folder and a file from %TEMP% (the checks are gates; Invoke on Cancel to the probe's answer is advisory: 100 ms)")
$checks = [ordered]@{}
foreach ($mode in @('folder', 'file')) {
    $title = "DISM Pro probe: choose a $mode"
    $report = Join-Path $Out "file-dialog-$mode.txt"
    $p = $null
    $owner = [IntPtr]::Zero
    try {
        if (-not (Test-Path $fileDialogExe)) { throw "missing $fileDialogExe (docker compose run --rm t3-tools builds it)" }
        Remove-Item $report -ErrorAction SilentlyContinue
        $p = Start-Process -FilePath $fileDialogExe -ArgumentList @('--mode', $mode, '--start', (Format-Arg $env:TEMP), '--out', (Format-Arg $report)) -PassThru
        $null = $p.Handle
        $shown = Wait-Until {
            $script:fileDialogOwner = [UiaGateWin32]::FindWindowW('dismpro.window', $fileDialogProbe)
            $script:fileDialogOwner -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:fileDialogOwner)
        } 30000
        if (-not $shown) { throw "the probe showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
        $owner = $script:fileDialogOwner
        $ownerRoot = $ae::FromHandle($owner)
        # The dialog: a Window named by its title, under its owner in UIA's tree or else the desktop.
        $named = [Windows.Automation.AndCondition]::new(
            [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $types::Window),
            [Windows.Automation.PropertyCondition]::new($ae::NameProperty, $title))
        $found = Wait-Until {
            $script:fileDialog = $ownerRoot.FindFirst([Windows.Automation.TreeScope]::Children, $named)
            if ($null -eq $script:fileDialog) { $script:fileDialog = $ae::RootElement.FindFirst([Windows.Automation.TreeScope]::Children, $named) }
            $null -ne $script:fileDialog
        } 15000
        $checks["${mode}: a Window `"$title`" shows (UIA)"] = $found
        if (-not $found) {
            if (Test-Path $report) { $lines += "  ${mode}: the probe answered with no dialog: $((Get-Content $report) -join ' ')" }
            throw "no Window `"$title`" in 15 s"
        }
        $dialog = $script:fileDialog
        $dialogHwnd = [IntPtr] $dialog.Current.NativeWindowHandle
        $checks["${mode}: owned by the probe's window, which it disables"] = [FileDialogGateWin32]::GetWindow($dialogHwnd, 4) -eq $owner -and -not [FileDialogGateWin32]::IsWindowEnabled($owner) # GW_OWNER
        # The probe's Button, a child of its window's element: an owned dialog may be one too, and
        # its own buttons further down.
        $statusButton = [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $types::Button)
        $status = { $b = $ownerRoot.FindFirst([Windows.Automation.TreeScope]::Children, $statusButton); if ($null -ne $b) { $b.Current.Name } }
        $checks["${mode}: the probe's tree answers while it's up"] = Wait-Until { (& $status) -eq 'pick: asked' } 2000
        $lines += Save-Screen ([PopupGateWin32]::Rect($dialogHwnd)) "file-dialog-$mode.png"
        $cancelId = [Windows.Automation.AndCondition]::new(
            [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $types::Button),
            [Windows.Automation.PropertyCondition]::new($ae::AutomationIdProperty, '2'))
        $hasCancel = Wait-Until {
            $script:fileDialogCancel = $dialog.FindFirst([Windows.Automation.TreeScope]::Descendants, $cancelId)
            if ($null -eq $script:fileDialogCancel) { $script:fileDialogCancel = Find-First $dialog $types::Button 'Cancel' }
            $null -ne $script:fileDialogCancel
        } 5000
        if (-not $hasCancel) { throw 'the dialog has no Cancel button (AutomationId 2, or named Cancel) in 5 s' }
        $cancel = $script:fileDialogCancel
        $sw = [Diagnostics.Stopwatch]::StartNew()
        (Get-Pattern $cancel ([Windows.Automation.InvokePattern]::Pattern)).Invoke()
        $answered = Wait-Until { Test-Path $report } 5000
        $answerMs = $sw.Elapsed.TotalMilliseconds
        $fields = @{}
        if ($answered) {
            foreach ($line in (Get-Content $report)) {
                $key, $value = $line -split '=', 2
                $fields[$key] = $value
            }
        }
        $lines += "  ${mode}: Invoke on Cancel to the answer $(Format-Ms $answerMs); report: $((Get-Content $report -ErrorAction SilentlyContinue) -join ' ')"
        $checks["${mode}: Cancel answers Ok(None) (picked=cancelled)"] = $answered -and $fields.picked -eq 'cancelled'
        $checks["${mode}: the modal loop dispatched the window's timers"] = $answered -and [int] $fields.ticks -ge 1
        $checks["${mode}: after it, the window is enabled and its tree says so"] = (Wait-Until { [FileDialogGateWin32]::IsWindowEnabled($owner) } 2000) -and (Wait-Until { (& $status) -eq 'pick: cancelled' } 2000)
    } catch {
        $lines += "${mode} FAILED: $($_.Exception.Message)"
        $checks["${mode}: client"] = $false
    } finally {
        if ($null -ne $p -and -not $p.HasExited) {
            if ($owner -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($owner, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
            if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += "the $mode probe did not close on WM_CLOSE"; $checks["${mode}: closes"] = $false }
        }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-62} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Composites: picker dialog' $lines $ok
if (-not $ok) { $failed++ }

# Composites: picker (TD-0009 A3.5 (12), A3.7 U19, A3.9, slice 6e) ---------------------------------
# G1's picker check through the app's path and U19's wall clock, on the picker probe
# (crates/dpro-ui/examples/picker.rs, fixtures/picker.rs) over a real folder: %TEMP% gets a folder
# holding one folder of 10,000 empty folders. First the probe as the app is: Enter on "Browse..."
# asks for the system dialog through platform::Host, which the window shows once the event is done:
# a Window "Choose a folder" owned by the probe's window; Invoke on its Cancel (AutomationId 2, else
# the Button "Cancel") leaves the page as it was ("No folder chosen", no sheet) with Browse...
# focused. Then the probe with --in-app, whose window answers every pick Unavailable as a WinPE
# without the dialog does: Enter on "Browse..." opens the in-app picker, the Group "Choose a folder"
# (a region landmark), with focus in its Edit "Path", whose Value is the folder; the DataGrid lists
# the one folder, listed by dpro_app::picker::FsLister's threads; Enter on its row opens it, and
# GridPattern's RowCount comes to all 10,000; Esc closes the sheet, and focus goes back to
# "Browse...". Those are gates. Enter to the first screen of rows (RowCount past the one row it
# had) and to all 10,000 are advisory (U19: the first screen <= 50 ms, U3), as the runner's NTFS is.
# A screenshot of the system dialog goes to -Out.
$pickerExe = Join-Path $Bin 'picker.exe'
$browseName = "Browse$([char]0x2026)"
$bigName = 'Ten thousand'
$pickerTree = Join-Path ([IO.Path]::GetTempPath()) "dpro-u19-$PID"
$lines = @("Picker on the picker probe at $Size over 10,000 folders under %TEMP%, with the system dialog and then --in-app (the checks are gates; the times are advisory: U19, Enter to the first screen of rows in UIA <= 50 ms)")
$checks = [ordered]@{}
$p = $null
$pickerHwnd = [IntPtr]::Zero
# Starts the picker probe ($script:p) with $Extra after its folder and size, and waits for its
# window ($script:pickerHwnd, $script:root) and its Button "Browse..." ($script:browse), focused.
function Start-PickerProbe([string[]] $Extra) {
    $script:pickerHwnd = [IntPtr]::Zero
    $script:p = Start-Process -FilePath $pickerExe -ArgumentList (@('--start', ('"{0}"' -f $pickerTree), '--size', $Size) + $Extra) -PassThru
    $null = $script:p.Handle
    $shown = Wait-Until {
        $script:pickerHwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro picker probe')
        $script:pickerHwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:pickerHwnd)
    } 30000
    if (-not $shown) { throw "the picker probe showed no window in 30 s ($(if ($script:p.HasExited) { "exited with code $($script:p.ExitCode)" } else { 'still running' }))" }
    $script:root = $ae::FromHandle($script:pickerHwnd)
    $script:browse = $null
    if (-not (Wait-Until { $script:browse = Find-First $script:root $types::Button $browseName; $null -ne $script:browse } 10000)) { throw "no Button `"$browseName`" in 10 s" }
    [void][GlassGateWin32]::Activate($script:pickerHwnd)
    $script:browse.SetFocus()
    if (-not (Wait-Until { $script:browse.Current.HasKeyboardFocus } 2000)) { throw "`"$browseName`" took no focus in 2 s" }
}
# Closes the picker probe ($script:p): WM_CLOSE, then Kill after 5 s.
function Stop-PickerProbe {
    if ($null -ne $script:p -and -not $script:p.HasExited) {
        if ($script:pickerHwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($script:pickerHwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $script:p.WaitForExit(5000)) { $script:p.Kill(); $script:lines += 'the picker probe did not close on WM_CLOSE'; $script:checks['closes'] = $false }
    }
    $script:p = $null
}
try {
    if (-not (Test-Path $pickerExe)) { throw "missing $pickerExe (docker compose run --rm t3-tools builds it)" }
    $big = Join-Path $pickerTree $bigName
    $sw = [Diagnostics.Stopwatch]::StartNew()
    foreach ($n in 0..9999) { $null = [IO.Directory]::CreateDirectory((Join-Path $big ('Folder {0:D5}' -f $n))) }
    $lines += 'made 10,000 folders in {0:N1} s' -f $sw.Elapsed.TotalSeconds
    # The app's path: the system dialog, asked for by the page and shown once the event is done.
    Start-PickerProbe @()
    Send-Key 0x0D
    $dialogNamed = [Windows.Automation.AndCondition]::new(
        [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $types::Window),
        [Windows.Automation.PropertyCondition]::new($ae::NameProperty, 'Choose a folder'))
    $pickerDialog = $null
    $dialogFound = Wait-Until {
        $script:pickerDialog = $root.FindFirst([Windows.Automation.TreeScope]::Children, $dialogNamed)
        if ($null -eq $script:pickerDialog) { $script:pickerDialog = $ae::RootElement.FindFirst([Windows.Automation.TreeScope]::Children, $dialogNamed) }
        $null -ne $script:pickerDialog
    } 15000
    $dialogHwnd = if ($dialogFound) { [IntPtr] $pickerDialog.Current.NativeWindowHandle } else { [IntPtr]::Zero }
    $checks['Enter on Browse shows the system dialog, owned by the probe'] = $dialogFound -and [FileDialogGateWin32]::GetWindow($dialogHwnd, 4) -eq $pickerHwnd # GW_OWNER
    if ($dialogFound) {
        $lines += Save-Screen ([PopupGateWin32]::Rect($dialogHwnd)) 'picker-dialog.png'
        $pickerCancelId = [Windows.Automation.AndCondition]::new(
            [Windows.Automation.PropertyCondition]::new($ae::ControlTypeProperty, $types::Button),
            [Windows.Automation.PropertyCondition]::new($ae::AutomationIdProperty, '2'))
        $pickerCancel = $null
        $hasCancel = Wait-Until {
            $script:pickerCancel = $pickerDialog.FindFirst([Windows.Automation.TreeScope]::Descendants, $pickerCancelId)
            if ($null -eq $script:pickerCancel) { $script:pickerCancel = Find-First $pickerDialog $types::Button 'Cancel' }
            $null -ne $script:pickerCancel
        } 5000
        if ($hasCancel) { (Get-Pattern $pickerCancel ([Windows.Automation.InvokePattern]::Pattern)).Invoke() }
        $checks['Cancel in it leaves the page as it was'] = $hasCancel -and (Wait-Until {
            $back = Find-First $root $types::Button $browseName
            (Find-Elements $root $types::Group 'Choose a folder').Count -eq 0 -and $null -ne (Find-First $root $types::Text 'No folder chosen') -and $null -ne $back -and $back.Current.HasKeyboardFocus
        } 5000)
    }
    Stop-PickerProbe
    # --in-app: no system dialog, so the page opens the in-app picker (A3.8's fallback).
    Start-PickerProbe @('--in-app')
    Send-Key 0x0D
    $sheet = $null
    $checks['Enter on Browse opens the Group "Choose a folder"'] = Wait-Until { $script:sheet = Find-First $root $types::Group 'Choose a folder'; $null -ne $script:sheet } 2000
    if ($null -eq $sheet) { throw 'no Group "Choose a folder" in 2 s' }
    $path = Find-First $sheet $types::Edit 'Path'
    if ($null -eq $path) { throw 'no Edit "Path" in the picker' }
    $checks['focus starts in its Edit "Path"'] = Wait-Until { $path.Current.HasKeyboardFocus } 2000
    $value = Get-Pattern $path ([Windows.Automation.ValuePattern]::Pattern)
    $checks['the Edit''s Value is the folder'] = Wait-Until { $value.Current.Value -eq $pickerTree } 2000
    $table = Find-First $sheet $types::DataGrid
    if ($null -eq $table) { throw 'no DataGrid in the picker' }
    $grid = Get-Pattern $table ([Windows.Automation.GridPattern]::Pattern)
    $checks['the DataGrid lists the one folder'] = Wait-Until { $grid.Current.RowCount -eq 1 } 10000
    $row = $null
    if (-not (Wait-Until { $script:row = Find-First $table $types::DataItem $bigName; $null -ne $script:row } 2000)) { throw "no DataItem `"$bigName`" in 2 s" }
    $row.SetFocus()
    if (-not (Wait-Until { $row.Current.HasKeyboardFocus } 2000)) { throw "`"$bigName`" took no focus in 2 s" }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Send-Key 0x0D
    $firstScreen = Wait-Until { $grid.Current.RowCount -gt 1 } 10000
    $firstMs = $sw.Elapsed.TotalMilliseconds
    $listed = $firstScreen -and (Wait-Until { $grid.Current.RowCount -eq 10000 } 10000)
    $allMs = $sw.Elapsed.TotalMilliseconds
    $checks['Enter on its row lists all 10,000 (within 10 s)'] = $listed
    if ($firstScreen) { $lines += "first screen: Enter to RowCount past the old row in UIA, $(Format-Ms $firstMs 50) (U19)" }
    if ($listed) { $lines += "all 10,000:   Enter to RowCount 10,000 in UIA, $(Format-Ms $allMs 1000)" }
    $lines += 'RowCount at the end: {0:N0}' -f $grid.Current.RowCount
    $path.SetFocus()
    $null = Wait-Until { $path.Current.HasKeyboardFocus } 2000
    Send-Key 0x1B
    $checks['Esc closes it'] = Wait-Until { (Find-Elements $root $types::Group 'Choose a folder').Count -eq 0 } 2000
    $checks['focus goes back to Browse'] = Wait-Until {
        $back = Find-First $root $types::Button $browseName
        $null -ne $back -and $back.Current.HasKeyboardFocus
    } 2000
} catch {
    $lines += "picker FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    Stop-PickerProbe
    try { if (Test-Path $pickerTree) { [IO.Directory]::Delete($pickerTree, $true) } } catch { $lines += "cleanup of $pickerTree failed: $($_.Exception.Message)" }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Composites: picker U19' $lines $ok
if (-not $ok) { $failed++ }

# Composites (TD-0009 A3.5, slice 6c) ---------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CompositesGateWin32 {
    [DllImport("user32.dll")]
    static extern bool SystemParametersInfoW(uint action, uint param, ref uint value, uint ini);
    // SPI_GETMESSAGEDURATION, in seconds; -1 if it couldn't be read.
    public static long MessageDuration() { uint v = 0; return SystemParametersInfoW(0x2016, 0, ref v, 0) ? v : -1; }
    // SPI_GETCLIENTAREAANIMATION, a BOOL; -1 if it couldn't be read.
    public static long ClientAreaAnimation() { uint v = 0; return SystemParametersInfoW(0x1042, 0, ref v, 0) ? v : -1; }
}
'@
$messageS = [CompositesGateWin32]::MessageDuration()
$messageMs = if ($messageS -gt 0) { $messageS * 1000 } else { 5000 }
$paletteField = 'Search commands'

$lines = @("Composites: slice 6c's palette and toast on the composites page (shell --composites) at $Size (the checks are gates, each read back within 2 s; the palette's opening is advisory: <= 100 ms from Ctrl+K to UIA seeing its field focused, U15)")
$lines += "Windows: SPI_GETMESSAGEDURATION $messageS s, SPI_GETCLIENTAREAANIMATION $([CompositesGateWin32]::ClientAreaAnimation())"
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size, '--composites') -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the composites page showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $found = Wait-Until { (Find-Elements $root $types::Text 'Clean').Count -ge 1 } 10000
    if (-not $found) { throw 'no heading "Clean" in 10 s' }
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)
    $m = [PopupGateWin32]::Rect($hwnd)
    $center = @([int] (($m[0] + $m[2]) / 2), [int] (($m[1] + $m[3]) / 2))
    [void][TextGateWin32]::MoveTo($center[0], $center[1])

    # The palette (A3.5 (3)): Ctrl+K shows it in an owned popup that takes the activation, its
    # field focused; Down moves the one selected result while focus stays in the field; typing
    # filters; Esc closes it and the page is active again.
    $palette = $null
    $sw = [Diagnostics.Stopwatch]::StartNew()
    [void][TextGateWin32]::Chord($ctrl, 0x4B) # Ctrl+K
    $opened = Wait-Until {
        $script:palette = Find-Popup $hwnd $types::Edit $paletteField
        $null -ne $script:palette -and (Test-Focused $types::Edit $paletteField)
    } 2000
    $openMs = $sw.Elapsed.TotalMilliseconds
    $checks['Ctrl+K: the palette, its field focused'] = $opened
    if (-not $opened) { throw "Ctrl+K showed no palette with its field focused in 2 s ($((Get-Shown $hwnd).Count) popups shown)" }
    $lines += "palette open: Ctrl+K to its field focused in UIA, $(Format-Ms $openMs)"
    $lines += Save-Screen ([PopupGateWin32]::Rect($palette.Hwnd)) 'composites-palette.png'
    $checks['it takes the activation'] = [PopupGateWin32]::GetForegroundWindow() -eq $palette.Hwnd
    $paletteRoot = $ae::FromHandle($palette.Hwnd)
    $options = { @(Find-Elements $paletteRoot $types::ListItem $null) }
    $selected = { @(& $options | Where-Object { Test-Selected $_ } | ForEach-Object { $_.Current.Name }) }
    $all = (& $options).Count
    $first = & $selected
    $lines += "  results: $all, selected: $($first -join ', ')"
    $checks['a List "Commands", exactly one result selected'] = (Find-Elements $paletteRoot $types::List 'Commands').Count -eq 1 -and $all -gt 1 -and $first.Count -eq 1
    Send-Key 0x28 # Down
    $checks['Down moves the selection, focus stays in the field'] = Wait-Until {
        $s = & $selected
        $s.Count -eq 1 -and $s[0] -ne $first[0] -and (Test-Focused $types::Edit $paletteField)
    } 2000
    [void][TextGateWin32]::Type('set')
    $checks['typing filters the results'] = Wait-Until {
        $n = (& $options).Count
        $value = (Get-Pattern $palette.Element ([Windows.Automation.ValuePattern]::Pattern)).Current.Value
        $value -eq 'set' -and $n -ge 1 -and $n -lt $all
    } 2000
    Send-Key 0x1B # Esc
    $checks['Esc closes it, the page active again'] = Wait-Until {
        (Get-Shown $hwnd).Count -eq 0 -and [PopupGateWin32]::GetForegroundWindow() -eq $hwnd
    } 2000

    # The toast (A3.5 (7)): Ctrl+J starts a job, whose toast shows at the window's bottom end
    # without the activation, and a click on it leaves the activation where it was. F6 gives it
    # the keyboard (Dismiss focused), and F6 hands it back. Ctrl+E ends the job: the toast hides
    # once Windows' message duration has passed with the pointer and the focus elsewhere.
    [void][TextGateWin32]::Chord($ctrl, 0x4A) # Ctrl+J
    $toast = $null
    $toastShown = Wait-Until { $script:toast = Find-Popup $hwnd $types::Button 'Dismiss'; $null -ne $script:toast } 2000
    $checks['Ctrl+J: a job''s toast shows'] = $toastShown
    if (-not $toastShown) { throw "Ctrl+J showed no toast with Dismiss in 2 s ($((Get-Shown $hwnd).Count) popups shown)" }
    Start-Sleep -Milliseconds 200
    $t = [PopupGateWin32]::Rect($toast.Hwnd)
    $m = [PopupGateWin32]::Rect($hwnd)
    $lines += Save-Screen $t 'composites-toast.png'
    $lines += "  toast at $($t -join ','), the window at $($m -join ',')"
    $checks['it shows without the activation'] = [PopupGateWin32]::GetForegroundWindow() -eq $hwnd -and ([PopupGateWin32]::ExStyle($toast.Hwnd) -band 0x08000000) -eq 0 # not WS_EX_NOACTIVATE
    $checks['at the window''s bottom end'] = ($m[2] - $t[2]) -ge 8 -and ($m[2] - $t[2]) -le 64 -and ($m[3] - $t[3]) -ge 8 -and ($m[3] - $t[3]) -le 64
    # In its left padding, halfway down: on the toast, on none of its controls.
    [void][TextGateWin32]::MoveTo($t[0] + 8, [int] (($t[1] + $t[3]) / 2))
    [void][PopupGateWin32]::Click($false)
    Start-Sleep -Milliseconds 300
    $checks['a click on it leaves the activation'] = [PopupGateWin32]::GetForegroundWindow() -eq $hwnd -and (Get-Shown $hwnd).Count -eq 1
    [void][TextGateWin32]::MoveTo($center[0], $center[1])
    Send-Key 0x75 # F6
    $checks['F6 gives it the keyboard, Dismiss focused'] = Wait-Until {
        [PopupGateWin32]::GetForegroundWindow() -eq $toast.Hwnd -and (Test-Focused $types::Button 'Dismiss')
    } 2000
    Send-Key 0x75 # F6
    $checks['F6 again hands it back'] = Wait-Until { [PopupGateWin32]::GetForegroundWindow() -eq $hwnd } 2000
    Send-Key 0x75 # F6
    $null = Wait-Until { [PopupGateWin32]::GetForegroundWindow() -eq $toast.Hwnd } 2000
    Send-Key 0x1B # Esc
    $checks['Esc in it hides it, the page active again'] = Wait-Until {
        (Get-Shown $hwnd).Count -eq 0 -and [PopupGateWin32]::GetForegroundWindow() -eq $hwnd
    } 2000
    [void][TextGateWin32]::Chord($ctrl, 0x45) # Ctrl+E
    $sw.Restart()
    $ended = Wait-Until { $null -ne (Find-Popup $hwnd $types::Button 'View') } 2000
    $hid = $ended -and (Wait-Until { (Get-Shown $hwnd).Count -eq 0 } ($messageMs + 3000))
    $lines += 'ended toast: hid {0:N0} ms after the job ended (message duration {1} ms)' -f $sw.Elapsed.TotalMilliseconds, $messageMs
    $checks['an ended toast hides after the message duration'] = $hid -and $sw.Elapsed.TotalMilliseconds -ge ($messageMs - 500)
} catch {
    $lines += "composites FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the composites page did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Composites' $lines $ok
if (-not $ok) { $failed++ }

# Accessibility depth: notifications (TD-0009 A4.5, A4.7 U23, slice 7b) ---------------------------
# Managed UI Automation can't hear notifications, so a COM IUIAutomation5 client does. It's declared
# flat below: IUIAutomation's 55 methods (ElementFromHandle the 4th), IUIAutomation2's 6, 3's 2 and
# 4's 2, then 5's two. It subscribes on an MTA thread of its own, as UI Automation advises for event
# handlers, and stamps each notification with the Stopwatch clock this script reads.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

[ComImport, Guid("C7CB2637-E6C2-4D0C-85DE-4948C02175C7"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface INoticeGateHandler {
    [PreserveSig] int HandleNotificationEvent(IntPtr sender, int kind, int processing,
        [MarshalAs(UnmanagedType.BStr)] string text, [MarshalAs(UnmanagedType.BStr)] string activity);
}

[ComImport, Guid("25F700C8-D816-4057-A9DC-3CBDEE77E256"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface INoticeGateAutomation5 {
    void M01(); void M02(); void M03();
    [PreserveSig] int ElementFromHandle(IntPtr hwnd, out IntPtr element);
    void M05(); void M06(); void M07(); void M08(); void M09(); void M10(); void M11(); void M12();
    void M13(); void M14(); void M15(); void M16(); void M17(); void M18(); void M19(); void M20();
    void M21(); void M22(); void M23(); void M24(); void M25(); void M26(); void M27(); void M28();
    void M29(); void M30(); void M31(); void M32(); void M33(); void M34(); void M35(); void M36();
    void M37(); void M38(); void M39(); void M40(); void M41(); void M42(); void M43(); void M44();
    void M45(); void M46(); void M47(); void M48(); void M49(); void M50(); void M51(); void M52();
    void M53(); void M54(); void M55(); void M56(); void M57(); void M58(); void M59(); void M60();
    void M61(); void M62(); void M63(); void M64(); void M65();
    [PreserveSig] int AddNotificationEventHandler(IntPtr element, int scope, IntPtr cacheRequest, INoticeGateHandler handler);
    [PreserveSig] int RemoveNotificationEventHandler(IntPtr element, INoticeGateHandler handler);
}

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public sealed class NoticeGateSink : INoticeGateHandler {
    public readonly ConcurrentQueue<string> Seen = new ConcurrentQueue<string>();
    public int HandleNotificationEvent(IntPtr sender, int kind, int processing, string text, string activity) {
        Seen.Enqueue(string.Format("{0}|{1}|{2}|{3}|{4}", Stopwatch.GetTimestamp(), kind, processing, activity, text));
        return 0;
    }
}

public static class NoticeGate {
    public static NoticeGateSink Sink;
    static Thread worker;
    static ManualResetEventSlim stop;
    static int hr;
    // Subscribes to the window's notifications (TreeScope_Subtree) and returns the HRESULT, 0 once
    // subscribed. Stop unsubscribes.
    public static int Start(IntPtr hwnd) {
        Sink = new NoticeGateSink();
        stop = new ManualResetEventSlim(false);
        var ready = new ManualResetEventSlim(false);
        hr = unchecked((int) 0x80004005);
        worker = new Thread(() => {
            IntPtr element = IntPtr.Zero;
            try {
                object o = Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("E22AD333-B25F-460C-83D0-0581107395C9")));
                var uia = (INoticeGateAutomation5) o;
                hr = uia.ElementFromHandle(hwnd, out element);
                if (hr == 0) { hr = uia.AddNotificationEventHandler(element, 7, IntPtr.Zero, Sink); }
                ready.Set();
                if (hr == 0) {
                    stop.Wait();
                    uia.RemoveNotificationEventHandler(element, Sink);
                }
            } catch (Exception e) {
                hr = e.HResult;
            } finally {
                ready.Set();
                if (element != IntPtr.Zero) { Marshal.Release(element); }
            }
        });
        worker.SetApartmentState(ApartmentState.MTA);
        worker.IsBackground = true;
        worker.Start();
        if (!ready.Wait(10000)) { return unchecked((int) 0x80004005); }
        return hr;
    }
    public static void Stop() {
        if (stop != null) { stop.Set(); }
        if (worker != null) { worker.Join(5000); worker = null; }
    }
}
'@

# The shell's drawer job runs 4 s from the drawer opening (--job-ms 4000), and Invoke on "Review
# changes" opens it. A job's progress is said on a new 10 % step, at least 2 s after its start and
# after its last notice: so once, at 2 s (50 %). At 4 s it's at 100 %, which is never said (A4.4).
# Gates: one progress notification, saying the step, of kind Other with MostRecent processing and
# the job's activity, not before 2 s. U23 (advisory): delivered <= 100 ms after its step was due.
$lines = @("Accessibility depth, notifications: the shell at $Size with --job-ms 4000, through a COM IUIAutomation5 client (the checks are gates; the time is advisory, U23: 100 ms)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size, '--job-ms', '4000') -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $review = $null
    $found = Wait-Until { $script:review = Find-First $root $types::Button 'Review changes'; $null -ne $script:review } 10000
    if (-not $found) { throw 'no button "Review changes" in 10 s' }
    $hr = [NoticeGate]::Start($hwnd)
    $checks['the client subscribes to notifications'] = $hr -eq 0
    if ($hr -ne 0) { throw ('AddNotificationEventHandler failed: 0x{0:X8}' -f $hr) }
    $t0 = [Diagnostics.Stopwatch]::GetTimestamp()
    (Get-Pattern $review ([Windows.Automation.InvokePattern]::Pattern)).Invoke()
    Start-Sleep -Milliseconds 5500
    [NoticeGate]::Stop()
    $seen = @([NoticeGate]::Sink.Seen.ToArray() | ForEach-Object {
        $f = $_ -split '\|', 5
        [pscustomobject]@{ Ms = ([long] $f[0] - $t0) * 1000.0 / [Diagnostics.Stopwatch]::Frequency; Kind = [int] $f[1]; Processing = [int] $f[2]; Activity = $f[3]; Text = $f[4] }
    })
    foreach ($n in $seen) { $lines += '  {0,8:N1} ms after Invoke: kind {1}, processing {2}, {3}: "{4}"' -f $n.Ms, $n.Kind, $n.Processing, $n.Activity, $n.Text }
    $progress = @($seen | Where-Object { $_.Activity -like 'DismPro.Progress.*' })
    $checks['one progress notification over the job'] = $progress.Count -eq 1
    if ($progress.Count -ge 1) {
        $n = $progress[0]
        $checks['it says the step: "Apply 20 changes: N0 percent"'] = $n.Text -match '^Apply 20 changes: [1-9]0 percent$'
        $checks['kind Other, MostRecent, DismPro.Progress.0'] = $n.Kind -eq 4 -and $n.Processing -eq 3 -and $n.Activity -eq 'DismPro.Progress.0'
        $checks['not before 2 s into the job'] = $n.Ms -ge 2000
        $lines += 'U23: delivered {0} after its step was due (2 s after Invoke)' -f (Format-Ms ($n.Ms - 2000))
    }
} catch {
    $lines += "notifications FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    [NoticeGate]::Stop()
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Accessibility depth: notifications' $lines $ok
if (-not $ok) { $failed++ }

# Accessibility depth: focus (TD-0009 A4.4, A4.7 U23, A4.9, slice 7c) ----------------------------
# Focus restoration on the drawer probe. Esc closes the drawer, and focus goes back to "Review
# changes", its opener (its scope's FocusMemo). Delete drops the focused first change, as a refresh
# would, and focus goes to the next, "Windows Update cache" (the successor rule). Both are gates,
# read back through UIA's FocusedElement within 2 s. U23 (advisory): the FocusChanged event to the
# opener arrives <= 100 ms after Esc. Managed UI Automation's focus handler hears it, registered on
# an MTA thread of its own as UI Automation advises for event handlers, and stamps each event with
# the Stopwatch clock this script reads. The class reaches UI Automation by reflection, through the
# types this script loaded: Add-Type can't reference its assemblies along with its default ones.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Reflection;
using System.Threading;

public static class FocusGate {
    public static ConcurrentQueue<string> Seen = new ConcurrentQueue<string>();
    static Thread worker;
    static ManualResetEventSlim stop;
    static string error;
    // Starts hearing FocusChanged anywhere on the desktop, through `automation` (managed UI
    // Automation's Automation class) and its handler type; returns null once registered, else why
    // not. Stop unregisters.
    public static string Start(Type automation, Type handlerType) {
        Seen = new ConcurrentQueue<string>();
        stop = new ManualResetEventSlim(false);
        var ready = new ManualResetEventSlim(false);
        error = "not registered in 10 s";
        worker = new Thread(() => {
            try {
                MethodInfo on = typeof(FocusGate).GetMethod("OnFocus", BindingFlags.Static | BindingFlags.NonPublic);
                Delegate handler = Delegate.CreateDelegate(handlerType, on);
                automation.GetMethod("AddAutomationFocusChangedEventHandler").Invoke(null, new object[] { handler });
                error = null;
                ready.Set();
                stop.Wait();
                automation.GetMethod("RemoveAutomationFocusChangedEventHandler").Invoke(null, new object[] { handler });
            } catch (Exception e) {
                if (error != null) { error = (e.InnerException ?? e).Message; }
            } finally {
                ready.Set();
            }
        });
        worker.SetApartmentState(ApartmentState.MTA);
        worker.IsBackground = true;
        worker.Start();
        ready.Wait(10000);
        return error;
    }
    public static void Stop() {
        if (stop != null) { stop.Set(); }
        if (worker != null) { worker.Join(5000); worker = null; }
    }
    // The handler: (object, AutomationFocusChangedEventArgs) binds to (object, EventArgs).
    static void OnFocus(object sender, EventArgs e) {
        long t = Stopwatch.GetTimestamp();
        string name = "";
        try {
            object current = sender.GetType().GetProperty("Current").GetValue(sender);
            name = (string) current.GetType().GetProperty("Name").GetValue(current);
        } catch (Exception) {
        }
        Seen.Enqueue(t + "|" + name);
    }
}
'@

$lines = @("Accessibility depth, focus: the drawer probe at $Size (the checks are gates, read back through UIA within 2 s; the time is advisory, U23: 100 ms)")
$checks = [ordered]@{}
$p = $null
$window = $null
try {
    if (-not (Test-Path $drawerExe)) { throw "missing $drawerExe (docker compose run --rm t3-tools builds it)" }
    $p = Start-Process -FilePath $drawerExe -ArgumentList @('--size', $Size) -PassThru
    $null = $p.Handle
    $window = Wait-DrawerWindow $p
    $null = Focus-Opener $window
    $why = [FocusGate]::Start([Windows.Automation.Automation], [Windows.Automation.AutomationFocusChangedEventHandler])
    $checks['the client hears FocusChanged'] = $null -eq $why
    if ($null -ne $why) { throw "AddAutomationFocusChangedEventHandler failed: $why" }
    Send-Key 0x0D # Enter
    if (-not (Wait-Until { Test-Focused $types::CheckBox 'Temporary files' } 2000)) { throw 'Enter on Review changes put no focus on the drawer''s first change in 2 s' }
    $t0 = [Diagnostics.Stopwatch]::GetTimestamp()
    Send-Key 0x1B # Esc
    $checks['Esc: focus goes back to "Review changes"'] = Wait-Until { Test-Focused $types::Button 'Review changes' } 2000
    $back = @([FocusGate]::Seen.ToArray() | ForEach-Object {
        $f = $_ -split '\|', 2
        [pscustomobject]@{ Ms = ([long] $f[0] - $t0) * 1000.0 / [Diagnostics.Stopwatch]::Frequency; Name = $f[1] }
    } | Where-Object { $_.Ms -ge 0 -and $_.Name -eq 'Review changes' } | Select-Object -First 1)
    if ($back.Count -eq 1) {
        $lines += 'U23: FocusChanged to "Review changes" {0} after Esc' -f (Format-Ms $back[0].Ms)
    } else {
        $lines += 'U23: no FocusChanged to "Review changes" heard after Esc (advisory)'
    }
    Send-Key 0x0D # Enter
    if (-not (Wait-Until { Test-Focused $types::CheckBox 'Temporary files' } 2000)) { throw 'the drawer opened again put no focus on its first change in 2 s' }
    Send-Key 0x2E # Delete: the probe drops the first change still listed
    $checks['Delete: focus goes to the next change, "Windows Update cache"'] = Wait-Until {
        (Test-Focused $types::CheckBox 'Windows Update cache') -and (Find-Elements $window.Root $types::CheckBox 'Temporary files').Count -eq 0
    } 2000
} catch {
    $lines += "focus FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    [FocusGate]::Stop()
    if ($null -ne $p -and -not $p.HasExited) {
        if ($null -ne $window) { [void][UiaGateWin32]::PostMessageW($window.Hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the drawer probe did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-62} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Accessibility depth: focus' $lines $ok
if (-not $ok) { $failed++ }

# Accessibility depth: menus (TD-0009 A4.5, A4.4, slice 7c) ----------------------------------------
# The adapter's PATCHES.md 12 and 13: MenuOpened and MenuClosed, ToolTipOpened and ToolTipClosed.
# As for notifications, a COM IUIAutomation client hears them. It reads each sender in its handler
# (a closed popup's node may be gone by then) and stamps the event with the Stopwatch clock this
# script reads. It's declared flat below: IUIAutomation's first 31 methods (GetRootElement the 3rd,
# AddAutomationEventHandler the 30th) and IUIAutomationElement's first 21 (ProcessId, ControlType
# and Name the 18th, 19th and 21st). A popup is a top-level window of its own, so the client listens
# on the desktop's subtree, on an MTA thread of its own.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

[ComImport, Guid("146C3C17-F12E-4E22-8C27-F894B9B79C69"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMenuGateHandler {
    [PreserveSig] int HandleAutomationEvent(IntPtr sender, int eventId);
}

[ComImport, Guid("D22108AA-8AC5-49A5-837B-37BBB3D7591E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMenuGateElement {
    void M01(); void M02(); void M03(); void M04(); void M05(); void M06(); void M07(); void M08();
    void M09(); void M10(); void M11(); void M12(); void M13(); void M14(); void M15(); void M16();
    void M17();
    [PreserveSig] int get_CurrentProcessId(out int pid);
    [PreserveSig] int get_CurrentControlType(out int type);
    void M20();
    [PreserveSig] int get_CurrentName([MarshalAs(UnmanagedType.BStr)] out string name);
}

[ComImport, Guid("30CBE57D-D9D0-452A-AB13-7AC5AC4825EE"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMenuGateAutomation {
    void M01(); void M02();
    [PreserveSig] int GetRootElement(out IntPtr root);
    void M04(); void M05(); void M06(); void M07(); void M08(); void M09(); void M10(); void M11();
    void M12(); void M13(); void M14(); void M15(); void M16(); void M17(); void M18(); void M19();
    void M20(); void M21(); void M22(); void M23(); void M24(); void M25(); void M26(); void M27();
    void M28(); void M29();
    [PreserveSig] int AddAutomationEventHandler(int eventId, IntPtr element, int scope, IntPtr cacheRequest, IMenuGateHandler handler);
    [PreserveSig] int RemoveAutomationEventHandler(int eventId, IntPtr element, IMenuGateHandler handler);
}

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public sealed class MenuGateSink : IMenuGateHandler {
    public readonly ConcurrentQueue<string> Seen = new ConcurrentQueue<string>();
    // Each event as "timestamp|event id|process id|control type|name"; -1, 0 and "" for what the
    // sender couldn't say.
    public int HandleAutomationEvent(IntPtr sender, int eventId) {
        long at = Stopwatch.GetTimestamp();
        int pid = -1, type = 0;
        string name = null;
        try {
            var e = (IMenuGateElement) Marshal.GetObjectForIUnknown(sender);
            int got;
            if (e.get_CurrentProcessId(out got) == 0) { pid = got; }
            if (e.get_CurrentControlType(out got) == 0) { type = got; }
            string n;
            if (e.get_CurrentName(out n) == 0) { name = n; }
            Marshal.ReleaseComObject(e);
        } catch (Exception) { }
        Seen.Enqueue(string.Format("{0}|{1}|{2}|{3}|{4}", at, eventId, pid, type, name ?? ""));
        return 0;
    }
}

public static class MenuGate {
    // MenuOpened, MenuClosed, ToolTipOpened, ToolTipClosed.
    static readonly int[] Events = { 20003, 20007, 20000, 20001 };
    public static MenuGateSink Sink;
    static Thread worker;
    static ManualResetEventSlim stop;
    static int hr;
    // Subscribes to the four events on the desktop (TreeScope_Subtree) and returns the HRESULT, 0
    // once subscribed to all four. Stop unsubscribes.
    public static int Start() {
        Sink = new MenuGateSink();
        stop = new ManualResetEventSlim(false);
        var ready = new ManualResetEventSlim(false);
        hr = unchecked((int) 0x80004005);
        worker = new Thread(() => {
            IntPtr root = IntPtr.Zero;
            try {
                object o = Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("E22AD333-B25F-460C-83D0-0581107395C9")));
                var uia = (IMenuGateAutomation) o;
                hr = uia.GetRootElement(out root);
                int added = 0;
                while (hr == 0 && added < Events.Length) {
                    hr = uia.AddAutomationEventHandler(Events[added], root, 7, IntPtr.Zero, Sink);
                    if (hr == 0) { added++; }
                }
                ready.Set();
                if (hr == 0) { stop.Wait(); }
                for (int k = 0; k < added; k++) { uia.RemoveAutomationEventHandler(Events[k], root, Sink); }
            } catch (Exception e) {
                hr = e.HResult;
            } finally {
                ready.Set();
                if (root != IntPtr.Zero) { Marshal.Release(root); }
            }
        });
        worker.SetApartmentState(ApartmentState.MTA);
        worker.IsBackground = true;
        worker.Start();
        if (!ready.Wait(10000)) { return unchecked((int) 0x80004005); }
        return hr;
    }
    public static void Stop() {
        if (stop != null) { stop.Set(); }
        if (worker != null) { worker.Join(5000); worker = null; }
    }
}
'@

# The events heard from `$ProcessId` since the Stopwatch timestamp `$From`, in order. A sender
# that couldn't say its process (-1) counts: a closed popup's node may be gone by then.
function Get-Heard([int] $ProcessId, [long] $From) {
    @([MenuGate]::Sink.Seen.ToArray() | ForEach-Object {
        $f = $_ -split '\|', 5
        [pscustomobject]@{ At = [long] $f[0]; Id = [int] $f[1]; Pid = [int] $f[2]; Type = [int] $f[3]; Name = $f[4] }
    } | Where-Object { $_.At -ge $From -and ($_.Pid -eq $ProcessId -or $_.Pid -eq -1) })
}
# The first event `$Id` heard from `$ProcessId` since `$From` (named `$Name` if given), waiting up
# to `$Ms`; $null if none.
function Wait-Heard([int] $ProcessId, [long] $From, [int] $Id, [string] $Name, [int] $Ms = 2000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do {
        $e = @(Get-Heard $ProcessId $From | Where-Object { $_.Id -eq $Id -and (-not $Name -or $_.Name -eq $Name) })
        if ($e.Count -gt 0) { return $e[0] }
        Start-Sleep -Milliseconds 10
    } while ($sw.ElapsedMilliseconds -lt $Ms)
    $null
}
function Get-Ms([long] $From, [long] $At) { ($At - $From) * 1000.0 / [Diagnostics.Stopwatch]::Frequency }
$eventName = @{ 20003 = 'MenuOpened'; 20007 = 'MenuClosed'; 20000 = 'ToolTipOpened'; 20001 = 'ToolTipClosed' }

# A popup's first open activates its window's adapter, whose answer to UIA's first ask is the open
# popup: no opened event then (PATCHES.md 12). So each kind is opened once first, and the gates are
# the close after it, the next open and its close. The keyboard's tip (A4.4) stays past autopop,
# ten delays, until a key: Esc.
$lines = @("Accessibility depth, menus: the shell at $Size, through a COM IUIAutomation client on the desktop (the events are gates, each heard within 2 s; their times are advisory, 100 ms)")
$checks = [ordered]@{}
$hwnd = [IntPtr]::Zero
$p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
$null = $p.Handle
try {
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s ($(if ($p.HasExited) { "exited with code $($p.ExitCode)" } else { 'still running' }))" }
    $root = $ae::FromHandle($hwnd)
    $rows = @()
    $found = Wait-Until { $script:rows = @(Get-Rows $root); $script:rows.Count -gt 4 } 10000
    if (-not $found) { throw 'no table rows in 10 s' }
    $row = $rows[3]
    $checks['the window is active'] = [GlassGateWin32]::Activate($hwnd)
    $hr = [MenuGate]::Start()
    $checks['the client subscribes to the 4 events'] = $hr -eq 0
    if ($hr -ne 0) { throw ('AddAutomationEventHandler failed: 0x{0:X8}' -f $hr) }
    $shellPid = $p.Id
    $t0 = [Diagnostics.Stopwatch]::GetTimestamp()

    # A row's menu: Shift+F10 on the focused row opens it, Esc closes it.
    $row.SetFocus()
    $focused = Wait-Until { $row.Current.HasKeyboardFocus } 2000
    [void][TextGateWin32]::Chord($shift, 0x79) # Shift+F10
    $first = $focused -and (Wait-Until { $null -ne (Find-Popup $hwnd $types::MenuItem 'Uninstall') } 2000)
    if (-not $first) { throw "Shift+F10 on '$($row.Current.Name)' showed no menu in 2 s (row focused: $focused)" }
    $t = [Diagnostics.Stopwatch]::GetTimestamp()
    Send-Key 0x1B # Esc
    $e = Wait-Heard $shellPid $t 20007
    $checks['Esc: MenuClosed'] = $null -ne $e
    if ($e) { $lines += "MenuClosed:    $(Format-Ms (Get-Ms $t $e.At)) after Esc" }
    [void](Wait-Until { (Get-Shown $hwnd).Count -eq 0 } 2000)
    $again = [Diagnostics.Stopwatch]::GetTimestamp()
    [void][TextGateWin32]::Chord($shift, 0x79)
    $e = Wait-Heard $shellPid $again 20003
    $checks['Shift+F10 again: MenuOpened on the Menu "Row"'] = $null -ne $e -and $e.Name -eq 'Row' -and $e.Type -eq 50009 # UIA_MenuControlTypeId
    if ($e) { $lines += "MenuOpened:    $(Format-Ms (Get-Ms $again $e.At)) after Shift+F10" }
    $reopened = Wait-Until { $null -ne (Find-Popup $hwnd $types::MenuItem 'Uninstall') } 2000
    $t = [Diagnostics.Stopwatch]::GetTimestamp()
    Send-Key 0x1B
    $e = Wait-Heard $shellPid $t 20007
    $checks['Esc again: MenuClosed'] = $reopened -and $null -ne $e
    Start-Sleep -Milliseconds 200 # a second event of either would be heard by now
    $menus = @(Get-Heard $shellPid $again | Where-Object { $_.Id -eq 20003 -or $_.Id -eq 20007 } | ForEach-Object { $_.Id })
    $checks['that open: one MenuOpened, then one MenuClosed'] = ($menus -join ',') -eq '20003,20007'

    # The tip on Review changes: the pointer resting on it, then leaving it.
    $delay = [int] [PopupGateWin32]::GetDoubleClickTime()
    $review = Find-First $root $types::Button 'Review changes'
    $b = $review.Current.BoundingRectangle
    $a = (Find-First $root $types::Text 'DISM Pro').Current.BoundingRectangle
    $on = @([int] ($b.X + $b.Width / 2), [int] ($b.Y + $b.Height / 2))
    $off = @([int] ($a.X + $a.Width / 2), [int] ($a.Y + $a.Height / 2))
    [void][TextGateWin32]::MoveTo($on[0], $on[1])
    $first = Wait-Until { $null -ne (Find-Popup $hwnd $types::ToolTip $tipText) } ($delay + 2000)
    if (-not $first) { throw "the pointer on Review changes showed no tip in $($delay + 2000) ms" }
    $t = [Diagnostics.Stopwatch]::GetTimestamp()
    [void][TextGateWin32]::MoveTo($off[0], $off[1])
    $e = Wait-Heard $shellPid $t 20001
    $checks['the pointer leaving: ToolTipClosed'] = $null -ne $e
    if ($e) { $lines += "ToolTipClosed: $(Format-Ms (Get-Ms $t $e.At)) after the pointer left" }
    [void](Wait-Until { $null -eq (Find-Popup $hwnd $types::ToolTip $tipText) } 2000)
    $t = [Diagnostics.Stopwatch]::GetTimestamp()
    [void][TextGateWin32]::MoveTo($on[0], $on[1])
    $e = Wait-Heard $shellPid $t 20000 '' ($delay + 2000)
    $checks['resting on it again: ToolTipOpened, the tip''s text'] = $null -ne $e -and $e.Name -eq $tipText -and $e.Type -eq 50022 # UIA_ToolTipControlTypeId
    if ($e) { $lines += "ToolTipOpened: $(Format-Ms (Get-Ms $t $e.At) ($delay + 100)) after the pointer came back (the delay: $delay ms; a tip just hidden shows again at once)" }
    $t = [Diagnostics.Stopwatch]::GetTimestamp()
    [void][TextGateWin32]::MoveTo($off[0], $off[1])
    $checks['leaving again: ToolTipClosed'] = $null -ne (Wait-Heard $shellPid $t 20001)

    # The keyboard's tip: UIA focuses Review changes (a client's focus shows no tip), then Tab and
    # Shift+Tab bring the keyboard's focus back to it, which shows its tip at once.
    [void][GlassGateWin32]::Activate($hwnd)
    $review.SetFocus()
    [void](Wait-Until { $review.Current.HasKeyboardFocus } 2000)
    Send-Key 0x09 # Tab
    $t = [Diagnostics.Stopwatch]::GetTimestamp()
    [void][TextGateWin32]::Chord($shift, 0x09) # Shift+Tab
    $e = Wait-Heard $shellPid $t 20000 $tipText
    $byKey = $null -ne $e -and $review.Current.HasKeyboardFocus
    $checks['Shift+Tab back to Review changes: ToolTipOpened'] = $byKey
    if ($byKey) {
        Start-Sleep -Milliseconds (10 * $delay + 1000)
        $early = @(Get-Heard $shellPid $e.At | Where-Object { $_.Id -eq 20001 }).Count
        $kept = $null -ne (Find-Popup $hwnd $types::ToolTip $tipText)
        $checks['it stays past autopop (10 delays): no ToolTipClosed'] = $kept -and $early -eq 0
        $t = [Diagnostics.Stopwatch]::GetTimestamp()
        Send-Key 0x1B # Esc
        $checks['Esc: ToolTipClosed'] = $null -ne (Wait-Heard $shellPid $t 20001)
    }
    foreach ($h in (Get-Heard $shellPid $t0)) {
        $lines += '  {0,8:N1} ms: {1}, control type {2}, process {3}, "{4}"' -f (Get-Ms $t0 $h.At), $eventName[$h.Id], $h.Type, $h.Pid, $h.Name
    }
} catch {
    $lines += "menus FAILED: $($_.Exception.Message)"
    $checks['client'] = $false
} finally {
    [MenuGate]::Stop()
    if (-not $p.HasExited) {
        if ($hwnd -ne [IntPtr]::Zero) { [void][UiaGateWin32]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
        if (-not $p.WaitForExit(5000)) { $p.Kill(); $lines += 'the shell did not close on WM_CLOSE'; $checks['closes'] = $false }
    }
}
foreach ($k in $checks.Keys) { $lines += '{0,-50} {1}' -f $k, (Format-Verdict $checks[$k]) }
$ok = $checks.Count -gt 0 -and -not ($checks.Values -contains $false)
Publish 'Accessibility depth: menus' $lines $ok
if (-not $ok) { $failed++ }

exit $failed
