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
      The checks are gates: names and roles (the page list, the table's rows, the check boxes),
      then Toggle, SelectionItem and Invoke through UIA, each read back through UIA.
  Live theme switch: see its section.
  Glass (ADR-0007, TD-0009 §5.2): the glass shell's window base, read off the screen, is DWM's
      backdrop and not the solid token. Switching transparency off shows the solid token, and on
      again brings the backdrop back, live, each within 2 s (advisory: 100 ms).

The probe target is dpro-ui's `shell` example until roadmap 3.2 gives dismpro.exe its shell.
Results go to annotations and the job summary; the raw reports go to -Out.
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

    $pages = @(Find-Elements $root $types::Group 'Pages' | ForEach-Object { Find-Elements $_ $types::ListItem })
    $rows = @(Find-Elements $root $types::Group 'Programs' | ForEach-Object { Find-Elements $_ $types::ListItem })
    $boxes = @(Find-Elements $root $types::CheckBox | ForEach-Object { $_.Current.Name })
    $checks['12 pages, Overview first'] = $pages.Count -eq 12 -and $pages[0].Current.Name -eq 'Overview'
    $checks['40 named rows'] = $rows.Count -eq 40 -and @($rows | Where-Object { $_.Current.Name }).Count -eq 40
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
        $item = Get-Pattern $rows[3] ([Windows.Automation.SelectionItemPattern]::Pattern)
        $item.Select()
        $checks['Select(row 4) selects it'] = Wait-Until { $item.Current.IsSelected }
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, IntPtr wp, string lp, uint flags, uint ms, out IntPtr result);

    // The mean brightness (0..1, Rec. 709 weights over sRGB) of a 16 x 10 grid over the client
    // area; -1 if unreadable.
    public static double Luminance(IntPtr hwnd) {
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
                    if (c == 0xFFFFFFFF) return -1;
                    sum += 0.2126 * ((c & 0xFF) / 255.0) + 0.7152 * (((c >> 8) & 0xFF) / 255.0) + 0.0722 * (((c >> 16) & 0xFF) / 255.0);
                    n++;
                }
            }
        } finally { ReleaseDC(hwnd, dc); }
        return sum / n;
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
    $p = Start-Process -FilePath $exe.shell -ArgumentList @('--size', $Size) -PassThru
    $null = $p.Handle
    $shown = Wait-Until {
        $script:hwnd = [UiaGateWin32]::FindWindowW('dismpro.window', 'DISM Pro shell probe')
        $script:hwnd -ne [IntPtr]::Zero -and [UiaGateWin32]::IsWindowVisible($script:hwnd)
    } 30000
    if (-not $shown) { throw "the shell showed no window in 30 s (exited: $($p.HasExited))" }
    $checks['starts light'] = Wait-Until { [ThemeGateWin32]::Luminance($hwnd) -gt 0.5 } 5000
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

exit $failed
