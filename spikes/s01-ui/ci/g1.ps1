<#
Spike S-01 (roadmap 1.6, ADR-0002), G1: the windows-2025 runner's interactive desktop (S-01 facts,
run 41589907500: session 2, DWM on, transparency on, 1024x768 at 96 DPI).

  1. Headless, on Windows: the pixel snapshots and the 100k-list fling for A and B, and the in-app
     acrylic cost over a 460x900 drawer (A's software path, and Direct2D on WARP; gate <= 2 ms and
     <= 4x a plain fill of the same region).
  2. Startup: process start to the first painted frame (the probe crate, the same hook for every
     exe), bare vs A (solid and glass) vs B, in interleaved rounds after a warm-up; medians; gate
     <= 3x bare.
     Then idle: working set and CPU time of each window left alone on the list (doc 04 budgets).
  3. UI Automation: names and roles for rows and buttons, and a click (Invoke) through UIA.
  4. Glass: Mica, the palette's system Acrylic and the in-app acrylic drawer, read off the screen over
     a magenta and then a green desktop (a backdrop must follow it), then with transparency switched
     off (the solid fallback, live, same process) and on again.

Results: annotations and the job summary; screenshots and raw output in -Out (an artifact).
#>
param(
    [string] $Bin = 'spikes/s01-ui/out/bin',
    [string] $Out = 'spikes/s01-ui/out/g1',
    [int] $Rounds = 9
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Bin = (Resolve-Path $Bin).Path
$null = New-Item -ItemType Directory -Force $Out
$Out = (Resolve-Path $Out).Path
$exe = @{ bare = Join-Path $Bin 'bare.exe'; a = Join-Path $Bin 's01-a.exe'; b = Join-Path $Bin 's01-b.exe' }
$title = @{ a = 'S-01 option A'; b = 'S-01 option B (Slint)' }

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

function Publish([string] $Title, [string[]] $Lines) {
    $text = $Lines -join "`n"
    Write-Host "== $Title`n$text"
    "== $Title`n$text`n" | Out-File (Join-Path $Out 'summary.txt') -Append
    if ($env:GITHUB_STEP_SUMMARY) { "### $Title`n``````n$text`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append }
    Write-Host "::notice title=$Title::$(Format-Annotation $text)"
}

# A harness error becomes an annotation (where and what): the job log needs a token to read.
trap {
    $at = $_.InvocationInfo
    Write-Host "::error title=S-01 G1 harness::$(Format-Annotation "line $($at.ScriptLineNumber): $("$($at.Line)".Trim())`n$($_.Exception.Message)")"
    exit 1
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms, UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class W {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    // The class as a pointer: PowerShell passes $null for a string parameter as "", which no class matches.
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(IntPtr cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetSysColors(int n, int[] elements, int[] colors);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool SystemParametersInfoW(uint action, uint param, string value, uint ini);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, string l, uint flags, uint timeout, out IntPtr result);
}
'@

# Runs an exe to completion with its output captured (GUI-subsystem exes too: the handles are passed).
function Invoke-Exe([string] $Path, [string[]] $Arguments, [int] $TimeoutSec = 900) {
    $log = Join-Path $Out ("{0}-{1}.log" -f [IO.Path]::GetFileNameWithoutExtension($Path), ($Arguments[0] -replace '^-+', ''))
    $p = Start-Process -FilePath $Path -ArgumentList $Arguments -NoNewWindow -PassThru `
        -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    $null = $p.Handle # keeps the exit code readable after exit
    if (-not $p.WaitForExit($TimeoutSec * 1000)) { $p.Kill(); throw "$Path $Arguments timed out" }
    $text = (Get-Content $log -Raw) + (Get-Content "$log.err" -Raw)
    [pscustomobject]@{ Code = $p.ExitCode; Lines = @("$text" -split "`r?`n" | Where-Object { $_ }) }
}

# 1. Headless --------------------------------------------------------------------------------------
$lines = @()
foreach ($o in 'a', 'b') {
    $r = Invoke-Exe $exe[$o] @('--snapshot', (Join-Path $Out "snap-$o"), '--scales', '1,1.5')
    $snaps = @($r.Lines | Where-Object { $_ -match '^snapshot ' })
    $pass = @($snaps | Where-Object { $_ -match ': (\d+)/(\d+) checks' -and $Matches[1] -eq $Matches[2] })
    $lines += "snapshots ${o}: $($pass.Count)/$($snaps.Count) screens pass every check (exit $($r.Code))"
    $lines += @($r.Lines | Where-Object { $_ -match 'FAIL' })
}
$lines += 'fling, software path (gate: p99 <= 16.6 ms)'
foreach ($o in 'a', 'b') {
    $r = Invoke-Exe $exe[$o] @('--bench', '--scales', '1,1.5', '--frames', (Join-Path $Out "frames-$o.tsv"))
    $lines += @($r.Lines | Where-Object { $_ -match '^bench ' })
    if ($r.Code) { $lines += "bench ${o}: exit $($r.Code)" }
}
$lines += 'in-app acrylic recomputation, 460x900 (gate: <= 2 ms on WARP, <= 4x a plain fill)'
$r = Invoke-Exe $exe.a @('--acrylic-bench', '--rounds', '31')
$lines += @($r.Lines | Where-Object { $_ -match '^acrylic ' })
if ($r.Code) { $lines += "acrylic bench: exit $($r.Code)" }
Publish 'S-01 G1 headless' $lines

# 2. Startup -------------------------------------------------------------------------------------
# The same window size for all, inside the 1024x768 screen.
$variants = [ordered]@{
    'bare'    = @($exe.bare, @())
    'A solid' = @($exe.a, @('--solid'))
    'A glass' = @($exe.a, @())
    'B'       = @($exe.b, @())
}
$names = @($variants.Keys)
$times = @{}
foreach ($n in $names) { $times[$n] = [Collections.Generic.List[double]]::new() }
$report = Join-Path $Out 'first-frame.txt'
for ($round = 0; $round -le $Rounds; $round++) {
    # Round 0 warms the file cache and is discarded; the order rotates every round.
    for ($k = 0; $k -lt $names.Count; $k++) {
        $n = $names[($k + $round) % $names.Count]
        Remove-Item $report -ErrorAction SilentlyContinue
        $arguments = @('--size', '960x640', '--first-frame', $report) + $variants[$n][1]
        $p = Start-Process -FilePath $variants[$n][0] -ArgumentList $arguments -PassThru
        if (-not $p.WaitForExit(60000)) { $p.Kill(); throw "$n didn't report a first frame in 60 s" }
        $us = if ((Test-Path $report) -and ((Get-Content $report -Raw) -match 'first_frame_us=(\d+)')) { [double] $Matches[1] } else { throw "$n wrote no first frame (exit $($p.ExitCode))" }
        if ($round -gt 0) { $times[$n].Add($us / 1000) }
        Start-Sleep -Milliseconds 300
    }
}
function Get-Median([double[]] $v) { $s = $v | Sort-Object; $s[[int][Math]::Floor(($s.Count - 1) / 2)] }
$base = Get-Median $times['bare']
$lines = @("process start to first painted frame, median of $Rounds interleaved rounds (gate: <= 3x bare)")
foreach ($n in $names) {
    $m = Get-Median $times[$n]
    $ratio = $m / $base
    $verdict = if ($n -eq 'bare') { '' } elseif ($ratio -le 3) { '  PASS' } else { '  FAIL' }
    $lines += '{0,-8} {1,7:N1} ms  ratio {2,5:N2}  (min {3:N1}, max {4:N1}){5}' -f $n, $m, $ratio, ($times[$n] | Measure-Object -Minimum).Minimum, ($times[$n] | Measure-Object -Maximum).Maximum, $verdict
}
Publish 'S-01 G1 startup' $lines

# Windows for sections 3 and 4 ---------------------------------------------------------------------
# Makes $H the foreground (active) window and says whether it is. The foreground lock refuses
# SetForegroundWindow to a process without recent input, so a synthetic Alt press comes first.
# Activation matters: DWM draws an inactive window's Mica (and Acrylic) as a solid color.
function Set-Foreground([IntPtr] $H) {
    for ($i = 0; $i -lt 5 -and [W]::GetForegroundWindow() -ne $H; $i++) {
        [W]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero) # VK_MENU down
        [W]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero) # and up (KEYEVENTF_KEYUP)
        [void][W]::SetForegroundWindow($H)
        Start-Sleep -Milliseconds 300
    }
    [W]::GetForegroundWindow() -eq $H
}

function Start-Window([string] $Path, [string[]] $Arguments, [string] $Title) {
    $p = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do {
        Start-Sleep -Milliseconds 100
        $h = [W]::FindWindowW([IntPtr]::Zero, $Title)
    } while ($h -eq [IntPtr]::Zero -and -not $p.HasExited -and $sw.ElapsedMilliseconds -lt 30000)
    if ($h -eq [IntPtr]::Zero) {
        $exited = $p.HasExited
        if (-not $exited) { $p.Kill() }
        throw "no window '$Title' in 30 s ($(if ($exited) { "exited, code $($p.ExitCode)" } else { 'killed' }))"
    }
    $active = Set-Foreground $h
    Start-Sleep -Milliseconds 1500 # first frames, and DWM's backdrop
    [pscustomobject]@{ Process = $p; Hwnd = $h; Active = $active }
}

function Stop-Window($w) {
    [void][W]::PostMessageW($w.Hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
    if (-not $w.Process.WaitForExit(5000)) { $w.Process.Kill() }
    Start-Sleep -Milliseconds 300
}

# 2b. Idle (doc 04: working set <= 40 MiB by GetProcessMemoryInfo, which .NET's WorkingSet64 reads;
# idle CPU 0 %). Each window on the 100k-row list, settled for 3 s, then 5 s untouched.
$lines = @('idle, List screen at 1000x700: settled 3 s, then 5 s untouched (budgets: working set <= 40 MiB, CPU 0 %)')
$idle = [ordered]@{
    'bare'    = @($exe.bare, @('--size', '1000x700'), 'S-01 bare')
    'A solid' = @($exe.a, @('--screen', 'list', '--size', '1000x700', '--solid'), $title.a)
    'A glass' = @($exe.a, @('--screen', 'list', '--size', '1000x700'), $title.a)
    'B'       = @($exe.b, @('--screen', 'list', '--size', '1000x700'), $title.b)
}
foreach ($n in $idle.Keys) {
    $w = $null
    try {
        $w = Start-Window $idle[$n][0] $idle[$n][1] $idle[$n][2]
        Start-Sleep -Milliseconds 1500 # Start-Window waited 1.5 s already
        $w.Process.Refresh()
        $cpu = $w.Process.TotalProcessorTime
        Start-Sleep -Seconds 5
        $w.Process.Refresh()
        $busy = ($w.Process.TotalProcessorTime - $cpu).TotalMilliseconds
        $mib = { param([long] $b) '{0,5:N1} MiB' -f ($b / 1MB) }
        $ws = $w.Process.WorkingSet64
        $lines += '{0,-8} working set {1} {2}; private {3}; peak working set {4}; CPU in 5 s idle {5:N0} ms' -f $n, (& $mib $ws), $(if ($ws -le 40MB) { 'PASS' } else { 'FAIL' }), (& $mib $w.Process.PrivateMemorySize64), (& $mib $w.Process.PeakWorkingSet64), $busy
    } catch {
        $lines += "${n}: FAILED: $($_.Exception.Message)"
    } finally {
        if ($w) { Stop-Window $w }
    }
}
Publish 'S-01 G1 idle' $lines

# 3. UI Automation ---------------------------------------------------------------------------------
$ua = [Windows.Automation.AutomationElement]
$types = [Windows.Automation.ControlType]
$any = [Windows.Automation.Condition]::TrueCondition
$lines = @()
foreach ($o in 'a', 'b') {
    $w = $null
    try {
        $w = Start-Window $exe[$o] @('--screen', 'drawer', '--size', '1000x700') $title[$o]
        $root = $ua::FromHandle($w.Hwnd)
        # AccessKit (both options) may answer the first query with a placeholder and build the tree
        # on the UI thread afterwards: poll until a button shows up, as a screen reader would wait
        # for the structure-changed event.
        $sw = [Diagnostics.Stopwatch]::StartNew()
        do {
            $all = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, $any))
            if (@($all | Where-Object { $_.Current.ControlType -eq $types::Button }).Count) { break }
            Start-Sleep -Milliseconds 250
        } while ($sw.ElapsedMilliseconds -lt 10000)
        $waited = $sw.ElapsedMilliseconds
        # Rows: the named list items of the list that has the most of them.
        $best = $null
        $bestRows = @()
        foreach ($list in @($all | Where-Object { $_.Current.ControlType -eq $types::List })) {
            $rows = @($list.FindAll([Windows.Automation.TreeScope]::Children, $any) |
                    Where-Object { $_.Current.ControlType -eq $types::ListItem -and $_.Current.Name })
            if ($rows.Count -gt $bestRows.Count) { $best = $list; $bestRows = $rows }
        }
        $buttons = @($all | Where-Object { $_.Current.ControlType -eq $types::Button -and $_.Current.Name } | ForEach-Object { $_.Current.Name })
        $rowsOk = $bestRows.Count -ge 10
        $buttonsOk = ($buttons -contains 'Cancel') -and ($buttons -contains 'Uninstall')
        $lines += "${o}: $($all.Count) elements (tree complete after $waited ms); rows: $($bestRows.Count) named ListItems in List '$(if ($best) { $best.Current.Name })' $(if ($rowsOk) { 'PASS' } else { 'FAIL' })"
        if ($bestRows) { $lines += "    e.g. $(($bestRows | Select-Object -First 3 | ForEach-Object { "'$($_.Current.Name)'" }) -join ', ')" }
        $lines += "    buttons: $(($buttons | Select-Object -Unique) -join ', ') $(if ($buttonsOk) { 'PASS' } else { 'FAIL' })"
        $roles = $all | Group-Object { $_.Current.ControlType.ProgrammaticName -replace '^ControlType\.', '' } | Sort-Object Count -Descending
        $lines += "    roles: $(($roles | ForEach-Object { "$($_.Name) $($_.Count)" }) -join ', ')"
        # A click through UIA: Cancel closes the drawer, so Uninstall disappears.
        $cancel = $all | Where-Object { $_.Current.ControlType -eq $types::Button -and $_.Current.Name -eq 'Cancel' } | Select-Object -First 1
        if ($cancel) {
            $invoke = $null
            if ($cancel.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref] $invoke)) {
                $invoke.Invoke()
                Start-Sleep -Milliseconds 800
                $after = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, $any) |
                        Where-Object { $_.Current.ControlType -eq $types::Button -and $_.Current.Name -eq 'Uninstall' })
                $lines += "    Invoke(Cancel): drawer $(if ($after.Count -eq 0) { 'closed PASS' } else { 'still open FAIL' })"
            } else {
                $lines += '    Invoke(Cancel): no Invoke pattern FAIL'
            }
        }
    } catch {
        $lines += "${o}: UI Automation FAILED: $($_.Exception.Message)"
    } finally {
        if ($w) { Stop-Window $w }
    }
}
Publish 'S-01 G1 UI Automation' $lines

# 4. Glass ---------------------------------------------------------------------------------------
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
function Set-Transparency([int] $On) {
    Set-ItemProperty $personalize EnableTransparency $On -Type DWord
    $r = [IntPtr]::Zero
    # WM_SETTINGCHANGE "ImmersiveColorSet" to every top-level window, as Settings does.
    [void][W]::SendMessageTimeoutW([IntPtr] 0xffff, 0x001A, [IntPtr]::Zero, 'ImmersiveColorSet', 2, 5000, [ref] $r)
    Start-Sleep -Milliseconds 2000
}
# A wallpaper of one color (Mica tints from the wallpaper image), and the same desktop color.
function Set-Desktop([Drawing.Color] $Color) {
    $s = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $path = Join-Path $Out ('wallpaper-{0:X2}{1:X2}{2:X2}.bmp' -f $Color.R, $Color.G, $Color.B)
    $bmp = [Drawing.Bitmap]::new($s.Width, $s.Height)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.Clear($Color)
    $g.Dispose()
    $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Bmp)
    $bmp.Dispose()
    [void][W]::SetSysColors(1, @(1), @([Drawing.ColorTranslator]::ToWin32($Color))) # COLOR_DESKTOP
    [void][W]::SystemParametersInfoW(0x0014, 0, $path, 3) # SPI_SETDESKWALLPAPER, update + broadcast
    Start-Sleep -Milliseconds 2500
}
$magenta = [Drawing.Color]::FromArgb(255, 0, 255)
$green = [Drawing.Color]::FromArgb(0, 192, 0)

function Get-Screen([string] $Name) {
    $s = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = [Drawing.Bitmap]::new($s.Width, $s.Height)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
    $g.Dispose()
    $bmp.Save((Join-Path $Out "$Name.png"), [Drawing.Imaging.ImageFormat]::Png)
    $bmp
}
# The mean color of a 7x7 block (acrylic has a noise texture).
function Get-Color($Bmp, [int] $X, [int] $Y) {
    $r = 0; $g = 0; $b = 0
    for ($dy = -3; $dy -le 3; $dy++) {
        for ($dx = -3; $dx -le 3; $dx++) {
            $c = $Bmp.GetPixel([Math]::Clamp($X + $dx, 0, $Bmp.Width - 1), [Math]::Clamp($Y + $dy, 0, $Bmp.Height - 1))
            $r += $c.R; $g += $c.G; $b += $c.B
        }
    }
    , @([int] ($r / 49), [int] ($g / 49), [int] ($b / 49))
}
function Format-Color($c) { 'rgb({0},{1},{2})' -f $c[0], $c[1], $c[2] }
# How magenta a color is: positive over the magenta desktop, negative over the green one.
function Get-Tint($c) { ($c[0] + $c[2]) / 2 - $c[1] }
function Get-Distance($c, $want) { [Math]::Max([Math]::Max([Math]::Abs($c[0] - $want[0]), [Math]::Abs($c[1] - $want[1])), [Math]::Abs($c[2] - $want[2])) }

# Probe points, client px (96 DPI on the runner): the window base in the content area (Mica), an
# empty stretch of the drawer (in-app acrylic), and an empty stretch of the palette (system Acrylic)
# placed over the bare desktop. Solid colors are the tokens (scene/src/tokens.rs).
$solid = @{
    light = @{ base = @(246, 247, 249); surface = @(255, 255, 255) }
    dark  = @{ base = @(15, 17, 21); surface = @(23, 26, 33) }
}
$client = @{ X = -250; Y = 60 }   # the window's client origin on screen: content and drawer visible
$paletteAt = @{ X = 640; Y = 200 } # mostly right of the window, over the desktop
function Get-Probes($w, [bool] $Palette) {
    $p = [W+POINT]::new()
    [void][W]::ClientToScreen($w.Hwnd, [ref] $p)
    $probes = [ordered]@{
        base   = @(($p.X + 700), ($p.Y + 80))
        drawer = @(($p.X + 900), ($p.Y + 650))
    }
    if ($Palette) { $probes.palette = @(($paletteAt.X + 330), ($paletteAt.Y + 236)) }
    $probes
}
function Move-Client($w) {
    $r = [W+RECT]::new()
    [void][W]::GetWindowRect($w.Hwnd, [ref] $r)
    $p = [W+POINT]::new()
    [void][W]::ClientToScreen($w.Hwnd, [ref] $p)
    # SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
    [void][W]::SetWindowPos($w.Hwnd, [IntPtr]::Zero, $client.X - ($p.X - $r.Left), $client.Y - ($p.Y - $r.Top), 0, 0, 0x15)
    Start-Sleep -Milliseconds 1500
}

function Test-Glass([string] $O, [string] $Theme, [string[]] $Extra, [bool] $Palette) {
    $res = @()
    Set-Desktop $magenta
    Set-Transparency 1
    $arguments = @('--screen', 'drawer', '--theme', $Theme, '--size', '1000x700') + $Extra
    if ($Palette) { $arguments += @('--palette', '--palette-at', "$($paletteAt.X),$($paletteAt.Y)") }
    $w = $null
    try {
        $w = Start-Window $exe[$O] $arguments $title[$O]
        Move-Client $w
        $probes = Get-Probes $w $Palette
        # The main window active: Mica, the in-app drawer, then the live fallback and back. Then,
        # with a palette, the palette active (as when it has the keyboard): its Acrylic over both.
        $steps = [ordered]@{
            'on, magenta' = @($w.Hwnd, { })
            'on, green'   = @($w.Hwnd, { Set-Desktop $green })
            'off (solid)' = @($w.Hwnd, { Set-Transparency 0 })
            'on again'    = @($w.Hwnd, { Set-Transparency 1 })
        }
        $popup = if ($Palette) { [W]::FindWindowW([IntPtr]::Zero, 'S01 palette') } else { [IntPtr]::Zero }
        if ($Palette -and $popup -eq [IntPtr]::Zero) { $res += '  no palette window' }
        if ($popup -ne [IntPtr]::Zero) {
            $steps['palette active, green'] = @($popup, { })
            $steps['palette active, magenta'] = @($popup, { Set-Desktop $magenta })
        }
        $phases = [ordered]@{}
        foreach ($s in $steps.Keys) {
            & $steps[$s][1]
            $active = Set-Foreground $steps[$s][0]
            Start-Sleep -Milliseconds 700 # the activation's backdrop transition
            $bmp = Get-Screen ('glass-{0}-{1}-{2}' -f $O, $Theme, ($s -replace '[^a-z]+', '-').Trim('-'))
            $phases[$s] = [ordered]@{}
            foreach ($k in $probes.Keys) { $phases[$s][$k] = Get-Color $bmp $probes[$k][0] $probes[$k][1] }
            $bmp.Dispose()
            $res += '  {0,-24} {1}{2}' -f $s, (($probes.Keys | ForEach-Object { "$_ $(Format-Color $phases[$s][$_])" }) -join '; '), $(if ($active) { '' } else { '  (NOT the active window)' })
        }
        $want = $solid[$Theme]
        $m = $phases['on, magenta']; $g = $phases['on, green']; $off = $phases['off (solid)']; $again = $phases['on again']
        $check = {
            param([string] $Name, [bool] $Ok, [string] $Detail)
            '  {0} {1}: {2}' -f $(if ($Ok) { 'PASS' } else { 'FAIL' }), $Name, $Detail
        }
        $res += & $check 'Mica follows the wallpaper' ((Get-Tint $m.base) - (Get-Tint $g.base) -gt 6) ('base tint {0:N0} over magenta vs {1:N0} over green' -f (Get-Tint $m.base), (Get-Tint $g.base))
        $res += & $check 'drawer is in-app acrylic, not solid' ((Get-Distance $m.drawer $want.surface) -gt 3) ('{0} vs solid {1}' -f (Format-Color $m.drawer), (Format-Color $want.surface))
        if ($phases.Contains('palette active, magenta')) {
            $pm = $phases['palette active, magenta']; $pg = $phases['palette active, green']
            $res += & $check 'palette Acrylic follows the desktop' ((Get-Tint $pm.palette) - (Get-Tint $pg.palette) -gt 6) ('tint {0:N0} over magenta vs {1:N0} over green' -f (Get-Tint $pm.palette), (Get-Tint $pg.palette))
        }
        $solidOk = ((Get-Distance $off.base $want.base) -le 3) -and ((Get-Distance $off.drawer $want.surface) -le 3)
        if ($Palette) { $solidOk = $solidOk -and ((Get-Distance $off.palette $want.surface) -le 3) }
        $res += & $check 'transparency off: solid fallback, live' $solidOk ('base {0} (want {1}), drawer {2} (want {3})' -f (Format-Color $off.base), (Format-Color $want.base), (Format-Color $off.drawer), (Format-Color $want.surface))
        # Back to what it showed before the switch (same wallpaper), and not the solid color.
        $backOk = ((Get-Distance $again.base $g.base) -le 3) -and ((Get-Distance $again.base $off.base) -ge 3)
        $res += & $check 'transparency on again: glass back, live' $backOk ('base {0} (before {1}, solid {2})' -f (Format-Color $again.base), (Format-Color $g.base), (Format-Color $off.base))
    } catch {
        $res += "  FAILED: $($_.Exception.Message)"
    } finally {
        if ($w) { Stop-Window $w }
        Set-Transparency 1
    }
    $res
}

# What decides whether DWM renders a backdrop live, besides activation: the session kind (ADR-0007
# falls back to solid in a remote session) and the display adapter.
$remote = [W]::GetSystemMetrics(0x1000) # SM_REMOTESESSION
$adapters = @(Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join ', '
$sessions = @(query session 2>&1 | ForEach-Object { "$_".Trim() } | Where-Object { $_ }) -join ' | '
$lines = @(
    "this session $((Get-Process -Id $PID).SessionId), SM_REMOTESESSION $remote; display: $adapters"
    "sessions: $sessions"
    'probes: rgb, 7x7 mean; wallpaper magenta, then green; then transparency off and on'
)
foreach ($theme in 'light', 'dark') {
    $lines += "A, $theme (Mica window, Acrylic palette popup, in-app acrylic drawer)"
    $lines += Test-Glass 'a' $theme @() $true
}
$lines += 'B, light, --glass (Mica requested on its winit window; no popup window)'
$lines += Test-Glass 'b' 'light' @('--glass') $false
Publish 'S-01 G1 glass' $lines
exit 0
