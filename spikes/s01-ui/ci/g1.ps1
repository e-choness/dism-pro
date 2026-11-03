<#
Spike S-01 (roadmap 1.6, ADR-0002), G1: the windows-2025 runner's interactive desktop (S-01 facts,
run 41589907500: session 2, DWM on, transparency on, 1024x768 at 96 DPI).

  1. Headless, on Windows: the pixel snapshots and the 100k-list fling for A and B, and the in-app
     acrylic cost over a 460x900 drawer (A's software path, and Direct2D on WARP; gate <= 2 ms and
     <= 4x a plain fill of the same region).
  2. Startup: process start to the first painted frame (the probe crate, the same hook for every
     exe), bare vs A (solid and glass) vs B, in interleaved rounds after a warm-up; medians; gate
     <= 3x bare.
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

Add-Type -AssemblyName System.Drawing, System.Windows.Forms, UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class W {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
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
function Start-Window([string] $Path, [string[]] $Arguments, [string] $Title) {
    $p = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do {
        Start-Sleep -Milliseconds 100
        $h = [W]::FindWindowW($null, $Title)
    } while ($h -eq [IntPtr]::Zero -and -not $p.HasExited -and $sw.ElapsedMilliseconds -lt 30000)
    if ($h -eq [IntPtr]::Zero) { throw "no window '$Title' (process exited: $($p.HasExited))" }
    [void][W]::SetForegroundWindow($h)
    Start-Sleep -Milliseconds 1500 # first frames, and DWM's backdrop
    [pscustomobject]@{ Process = $p; Hwnd = $h }
}

function Stop-Window($w) {
    [void][W]::PostMessageW($w.Hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
    if (-not $w.Process.WaitForExit(5000)) { $w.Process.Kill() }
    Start-Sleep -Milliseconds 300
}

# 3. UI Automation ---------------------------------------------------------------------------------
$ua = [Windows.Automation.AutomationElement]
$types = [Windows.Automation.ControlType]
$lines = @()
foreach ($o in 'a', 'b') {
    $w = Start-Window $exe[$o] @('--screen', 'drawer', '--size', '1000x700') $title[$o]
    try {
        $root = $ua::FromHandle($w.Hwnd)
        $all = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition))
        # Rows: the named list items of the list that has the most of them.
        $best = $null
        $bestRows = @()
        foreach ($list in @($all | Where-Object { $_.Current.ControlType -eq $types::List })) {
            $rows = @($list.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition) |
                    Where-Object { $_.Current.ControlType -eq $types::ListItem -and $_.Current.Name })
            if ($rows.Count -gt $bestRows.Count) { $best = $list; $bestRows = $rows }
        }
        $buttons = @($all | Where-Object { $_.Current.ControlType -eq $types::Button -and $_.Current.Name } | ForEach-Object { $_.Current.Name })
        $rowsOk = $bestRows.Count -ge 10
        $buttonsOk = ($buttons -contains 'Cancel') -and ($buttons -contains 'Uninstall')
        $lines += "${o}: $($all.Count) elements; rows: $($bestRows.Count) named ListItems in List '$(if ($best) { $best.Current.Name })' $(if ($rowsOk) { 'PASS' } else { 'FAIL' })"
        if ($bestRows) { $lines += "    e.g. $(($bestRows | Select-Object -First 3 | ForEach-Object { "'$($_.Current.Name)'" }) -join ', ')" }
        $lines += "    buttons: $(($buttons | Select-Object -Unique) -join ', ') $(if ($buttonsOk) { 'PASS' } else { 'FAIL' })"
        # A click through UIA: Cancel closes the drawer, so Uninstall disappears.
        $cancel = $all | Where-Object { $_.Current.ControlType -eq $types::Button -and $_.Current.Name -eq 'Cancel' } | Select-Object -First 1
        if ($cancel) {
            $invoke = $null
            if ($cancel.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref] $invoke)) {
                $invoke.Invoke()
                Start-Sleep -Milliseconds 800
                $after = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
                        Where-Object { $_.Current.ControlType -eq $types::Button -and $_.Current.Name -eq 'Uninstall' })
                $lines += "    Invoke(Cancel): drawer $(if ($after.Count -eq 0) { 'closed' } else { 'still open' })"
            } else {
                $lines += '    Invoke(Cancel): no Invoke pattern'
            }
        }
    } catch {
        $lines += "${o}: UI Automation FAILED: $($_.Exception.Message)"
    } finally {
        Stop-Window $w
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
function Set-Desktop([int] $Colorref) {
    [void][W]::SetSysColors(1, @(1), @($Colorref)) # COLOR_DESKTOP
    Start-Sleep -Milliseconds 2000
}
# No wallpaper: the desktop is its solid color, which the backdrops sample.
[void][W]::SystemParametersInfoW(0x0014, 0, '', 3) # SPI_SETDESKWALLPAPER, update + broadcast
$magenta = 0x00FF00FF
$green = 0x0000C000

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
    $w = Start-Window $exe[$O] $arguments $title[$O]
    try {
        Move-Client $w
        $probes = Get-Probes $w $Palette
        $phases = [ordered]@{}
        $steps = [ordered]@{
            'on, magenta'   = { }
            'on, green'     = { Set-Desktop $green }
            'off (solid)'   = { Set-Transparency 0 }
            'on again'      = { Set-Transparency 1 }
        }
        foreach ($s in $steps.Keys) {
            & $steps[$s]
            $bmp = Get-Screen ("glass-{0}-{1}-{2}" -f $O, $Theme, ($s -replace '[^a-z]+', '-').Trim('-'))
            $phases[$s] = [ordered]@{}
            foreach ($k in $probes.Keys) { $phases[$s][$k] = Get-Color $bmp $probes[$k][0] $probes[$k][1] }
            $bmp.Dispose()
            $res += "  {0,-12} {1}" -f $s, (($probes.Keys | ForEach-Object { "$_ $(Format-Color $phases[$s][$_])" }) -join '; ')
        }
        $want = $solid[$Theme]
        $m = $phases['on, magenta']; $g = $phases['on, green']; $off = $phases['off (solid)']; $again = $phases['on again']
        $check = {
            param([string] $Name, [bool] $Ok, [string] $Detail)
            "  {0} {1}: {2}" -f $(if ($Ok) { 'PASS' } else { 'FAIL' }), $Name, $Detail
        }
        $res += & $check 'Mica follows the desktop' ((Get-Tint $m.base) - (Get-Tint $g.base) -gt 6) ("base tint {0:N0} over magenta vs {1:N0} over green" -f (Get-Tint $m.base), (Get-Tint $g.base))
        $res += & $check 'drawer is in-app acrylic, not solid' ((Get-Distance $m.drawer $want.surface) -gt 3) ("{0} vs solid {1}" -f (Format-Color $m.drawer), (Format-Color $want.surface))
        if ($Palette) {
            $res += & $check 'palette Acrylic follows the desktop' ((Get-Tint $m.palette) - (Get-Tint $g.palette) -gt 6) ("tint {0:N0} over magenta vs {1:N0} over green" -f (Get-Tint $m.palette), (Get-Tint $g.palette))
        }
        $solidOk = ((Get-Distance $off.base $want.base) -le 3) -and ((Get-Distance $off.drawer $want.surface) -le 3)
        if ($Palette) { $solidOk = $solidOk -and ((Get-Distance $off.palette $want.surface) -le 3) }
        $res += & $check 'transparency off: solid fallback, live' $solidOk ("base {0} (want {1}), drawer {2} (want {3})" -f (Format-Color $off.base), (Format-Color $want.base), (Format-Color $off.drawer), (Format-Color $want.surface))
        $res += & $check 'transparency on again: glass back' ((Get-Distance $again.base $want.base) -gt 6) ("base {0}" -f (Format-Color $again.base))
    } catch {
        $res += "  FAILED: $($_.Exception.Message)"
    } finally {
        Stop-Window $w
        Set-Transparency 1
    }
    $res
}

$lines = @('probes: rgb, 7x7 mean; desktop magenta, then green; then transparency off and on')
foreach ($theme in 'light', 'dark') {
    $lines += "A, $theme (Mica window, Acrylic palette popup, in-app acrylic drawer)"
    $lines += Test-Glass 'a' $theme @() $true
}
$lines += 'B, light, --glass (Mica requested on its winit window; no popup window)'
$lines += Test-Glass 'b' 'light' @('--glass') $false
Publish 'S-01 G1 glass' $lines
exit 0
