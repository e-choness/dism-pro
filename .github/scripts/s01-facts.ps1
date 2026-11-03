<#
Spike S-01 (roadmap 1.6, ADR-0002), first pass: the facts the UI toolkit choice rests on, gathered on
the windows-2025 runner. Throwaway: lives on diag/s01* only.

  1. Which UI-relevant DLLs base WinPE ships (the ADK's winpe.wim lists, amd64 + arm64): decides
     whether option A can lean on Direct2D/DirectWrite/DirectComposition in WinPE, and what any
     toolkit may import.
  2. Whether the hosted runner has a real desktop for the G1 gates: DWM composition, a shown window
     reaching the screen, UI Automation over it, and the Mica backdrop attribute.

Results go to annotations (readable without signing in) and the job summary.
#>
param([string] $WinPE = 'winpe-files')

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

function Publish([string] $Title, [string] $Text) {
    Write-Host "== $Title`n$Text"
    if ($env:GITHUB_STEP_SUMMARY) { "### $Title`n``````n$Text`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append }
    Write-Host "::notice title=$Title::$(Format-Annotation $Text)"
}

# 1. WinPE --------------------------------------------------------------------------------------
$ui = @(
    'user32', 'gdi32', 'gdi32full', 'gdiplus', 'msimg32', 'usp10', 'lpk', 'comctl32', 'uxtheme', 'dwmapi',
    'd2d1', 'dwrite', 'd3d11', 'd3d10warp', 'd3d12', 'dxgi', 'dxcore', 'dcomp', 'd3dcompiler_47',
    'opengl32', 'glu32', 'vulkan-1', 'windowscodecs', 'uiautomationcore', 'oleacc', 'ole32',
    'oleaut32', 'combase', 'imm32', 'msctf', 'textinputframework', 'shcore', 'shell32', 'shlwapi',
    'directmanipulation', 'twinapi.appcore', 'coremessaging', 'coreuicomponents', 'dwmcore',
    'winmm', 'version', 'setupapi', 'cfgmgr32', 'powrprof', 'wtsapi32', 'userenv', 'ws2_32'
)
$lines = @()
foreach ($arch in 'amd64', 'arm64') {
    $list = Join-Path $WinPE "$arch.txt"
    if (-not (Test-Path $list)) { $lines += "${arch}: no list at $list (cache miss)"; continue }
    $have = @{}
    Get-Content $list | ForEach-Object { $have[$_.Trim().ToLowerInvariant()] = $true }
    $present = @($ui | Where-Object { $have["$_.dll"] })
    $missing = @($ui | Where-Object { -not $have["$_.dll"] })
    $lines += "${arch} ($($have.Count) DLLs in base WinPE System32)"
    $lines += "  present: $($present -join ', ')"
    $lines += "  missing: $($missing -join ', ')"
}
Publish 'S-01 WinPE UI DLLs' ($lines -join "`n")

# 2. Desktop ------------------------------------------------------------------------------------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Desk {
    [DllImport("dwmapi.dll")] public static extern int DwmIsCompositionEnabled(out bool enabled);
    [DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
    [DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr desk);
    [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
}
'@
$d = @()
try {
    $d += "process session $((Get-Process -Id $PID).SessionId), console session $([Desk]::WTSGetActiveConsoleSessionId()), UserInteractive $([Environment]::UserInteractive), user $env:USERNAME"
    $desk = [Desk]::OpenInputDesktop(0, $false, 0x0001)
    $d += "input desktop: $(if ($desk -ne [IntPtr]::Zero) { [void][Desk]::CloseDesktop($desk); 'open' } else { "not available (error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))" })"
    $d += "sessions: $((query session 2>&1 | Out-String).Trim() -replace '\s+', ' ')"
    $d += "dwm.exe running: $([bool](Get-Process dwm -ErrorAction SilentlyContinue))"
    $on = $false
    $hr = [Desk]::DwmIsCompositionEnabled([ref] $on)
    $d += "DwmIsCompositionEnabled: hr $hr, $on"
    $t = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -ErrorAction SilentlyContinue
    $d += "EnableTransparency (HKCU): $(if ($t) { $t.EnableTransparency } else { 'absent' }); AppsUseLightTheme: $(if ($t) { $t.AppsUseLightTheme } else { 'absent' })"
    $d += "screen $([Desk]::GetSystemMetrics(0))x$([Desk]::GetSystemMetrics(1)), system DPI $([Desk]::GetDpiForSystem())"
} catch {
    $d += "FAILED: $($_.Exception.Message)"
}

# A real window: does it reach the screen, does UI Automation see it, does DWM take Mica?
try {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
    $form = [Windows.Forms.Form]::new()
    $form.Text = 'S01 probe'
    $form.StartPosition = 'Manual'
    $form.Location = [Drawing.Point]::new(100, 100)
    $form.Size = [Drawing.Size]::new(400, 300)
    $form.BackColor = [Drawing.Color]::FromArgb(255, 32, 160, 96)
    $form.TopMost = $true
    $button = [Windows.Forms.Button]::new()
    $button.Text = 'S01 button'
    $button.Location = [Drawing.Point]::new(20, 20)
    $form.Controls.Add($button)
    $form.Show()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 1500) { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 }

    $client = $form.PointToScreen([Drawing.Point]::new(200, 200))
    $bmp = [Drawing.Bitmap]::new(1, 1)
    $g = [Drawing.Graphics]::FromImage($bmp)
    try {
        $g.CopyFromScreen($client, [Drawing.Point]::Empty, [Drawing.Size]::new(1, 1))
        $px = $bmp.GetPixel(0, 0)
        $d += "screen pixel inside the window: rgb($($px.R), $($px.G), $($px.B)) (window is rgb(32, 160, 96))"
    } catch {
        $d += "CopyFromScreen failed: $($_.Exception.Message)"
    } finally { $g.Dispose(); $bmp.Dispose() }

    $root = [Windows.Automation.AutomationElement]::RootElement
    $cond = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'S01 probe')
    $win = $root.FindFirst([Windows.Automation.TreeScope]::Children, $cond)
    if ($win) {
        $kids = $win.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
        $names = @($kids | ForEach-Object { "$($_.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''):'$($_.Current.Name)'" })
        $d += "UI Automation: window found; descendants: $($names -join ', ')"
    } else {
        $d += "UI Automation: window NOT found among $($root.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition).Count) top-level elements"
    }

    # DWMWA_SYSTEMBACKDROP_TYPE (38) = DWMSBT_MAINWINDOW (2): Mica; DWMWA_USE_IMMERSIVE_DARK_MODE (20).
    $v = 2
    $hr = [Desk]::DwmSetWindowAttribute($form.Handle, 38, [ref] $v, 4)
    $d += "DwmSetWindowAttribute(SYSTEMBACKDROP_TYPE = Mica): hr 0x$('{0:X8}' -f $hr)"
    $v = 1
    $hr = [Desk]::DwmSetWindowAttribute($form.Handle, 20, [ref] $v, 4)
    $d += "DwmSetWindowAttribute(USE_IMMERSIVE_DARK_MODE): hr 0x$('{0:X8}' -f $hr)"
    $form.Close()
} catch {
    $d += "window probe FAILED: $($_.Exception.Message)"
}
$os = Get-CimInstance Win32_OperatingSystem
$d += "OS: $($os.Caption) $($os.Version); edition features: Server-Gui-Shell installed $([bool](Get-WindowsFeature Server-Gui-Shell -ErrorAction SilentlyContinue | Where-Object Installed))"
Publish 'S-01 runner desktop' ($d -join "`n")
exit 0
