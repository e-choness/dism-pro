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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, IntPtr wp, string lp, uint flags, uint ms, out IntPtr result);

    // The grid points the last Luminance call couldn't read (CLR_INVALID: outside the DC's clip,
    // for example under another window).
    public static int Unreadable;

    // The mean brightness (0..1, Rec. 709 weights over sRGB) of the readable points of a 16 x 10
    // grid over the client area; -1 if none is readable.
    public static double Luminance(IntPtr hwnd) {
        Unreadable = 0;
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
                    sum += 0.2126 * ((c & 0xFF) / 255.0) + 0.7152 * (((c >> 8) & 0xFF) / 255.0) + 0.0722 * (((c >> 16) & 0xFF) / 255.0);
                    n++;
                }
            }
        } finally { ReleaseDC(hwnd, dc); }
        return n == 0 ? -1 : sum / n;
    }
}
'@
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
    # SMTO_ABORTIFHUNG; our window only, so the runner's other windows aren't involved.
    [void][ThemeGateWin32]::SendMessageTimeoutW($Hwnd, 0x001A, [IntPtr]::Zero, 'ImmersiveColorSet', 2, 2000, [ref] $result)
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $script:startLum = -1
    $checks['starts light'] = Wait-Until { $script:startLum = [ThemeGateWin32]::Luminance($hwnd); $script:startLum -gt 0.5 } 5000
    $lines += '{0,-12} {1,7:N1} ms  luminance {2:N2}{3}' -f 'light', $sw.Elapsed.TotalMilliseconds, $script:startLum, $(if ([ThemeGateWin32]::Unreadable -gt 0) { " ($([ThemeGateWin32]::Unreadable) of 160 points unreadable)" } else { '' })
    foreach ($step in @(@{ Name = 'dark'; Light = 0 }, @{ Name = 'light again'; Light = 1 })) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        Set-AppMode $step.Light $hwnd
        $switched = Wait-Until {
            $l = [ThemeGateWin32]::Luminance($hwnd)
            $l -ge 0 -and (($step.Light -eq 0 -and $l -lt 0.5) -or ($step.Light -eq 1 -and $l -gt 0.5))
        } 2000
        $ms = $sw.Elapsed.TotalMilliseconds
        $lines += '{0,-12} {1}  luminance {2:N2}{3}' -f $step.Name, $(if ($switched) { '{0,7:N1} ms' -f $ms } else { 'NOT in 2 s' }), [ThemeGateWin32]::Luminance($hwnd), $(if ($switched -and $ms -gt 100) { ' (advisory: over 100 ms)' } else { '' })
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
    if (-not $shown) { throw "the glass shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
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
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
    $root = $ae::FromHandle($hwnd)
    $size = "37${oldDecimal}0 MB"
    $day = [ValuesGateWin32]::Date(2026, 1, 2)
    $lines += "the runner's formats: decimal '$oldDecimal', short date '$oldDate' ($day)"
    $names = @()
    $read = Wait-Until { $script:names = Get-CellNames $root $valueRow; $script:names.Count -gt 0 } 10000
    if (-not $read) { throw "no row ""$valueRow"" in 10 s" }
    $lines += "  $valueRow`: $($names -join ' | ')"
    $checks['a size reads in the user''s decimal separator'] = $names -contains $size
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
    $checks['the old formats come back'] = $restored -and (Wait-Until { $n = Get-CellNames $root $valueRow; ($n -contains $size) -and ($n -contains $day) } 2000)
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

exit $failed
