<#
TD-0009 §7, §10: G1's UI gates, on the windows-2025 runner's interactive desktop (1024x768 at
96 DPI; S-01 run 41589907500). Every gate is a ratio or a count against dpro-win's bare_window, the
raw-API baseline, measured in this job, so it holds on whatever machine the runner lands on
(doc 04 §3).

  U4  Startup: process start to the first painted frame (--ui-probe first-frame), bare_window and
      the probe target in interleaved rounds after a discarded warm-up round. Gate: median
      <= 3x bare_window's.
  U6  Idle: from the first frame, 3 s to settle, then 5 s untouched (--ui-probe idle). Gate: no
      timer and no paint in those 5 s. CPU time and messages are reported next to bare_window's.
  U7  Idle working set. Gate: <= 40 MiB (doc 04 §2), at the 1000x700 window that fits the runner's
      screen. The budget's 1280x800 default adds 1.1 MiB of canvas.

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
$names = @($exe.Keys)
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
    $arguments = @('--size', $Size, '--ui-probe', $Kind, '--probe-out', $report)
    $p = Start-Process -FilePath $exe[$Name] -ArgumentList $arguments -PassThru
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
    $fields
}

function Get-Median([double[]] $v) { $s = $v | Sort-Object; $s[[int][Math]::Floor(($s.Count - 1) / 2)] }
function Format-Verdict([bool] $Ok) { if ($Ok) { 'PASS' } else { 'FAIL' } }

# U4 startup ---------------------------------------------------------------------------------------
$times = @{}
foreach ($n in $names) { $times[$n] = [Collections.Generic.List[double]]::new() }
for ($round = 0; $round -le $Rounds; $round++) {
    # Round 0 warms the file cache and is discarded; the order alternates every round.
    for ($k = 0; $k -lt $names.Count; $k++) {
        $n = $names[($k + $round) % $names.Count]
        $r = Invoke-Probe $n 'first-frame'
        if ($round -gt 0) { $times[$n].Add($r.first_frame_us / 1000) }
        Start-Sleep -Milliseconds 300
    }
}
$lines = @("process start to the first painted frame at $Size, median of $Rounds interleaved rounds (gate: <= 3x bare_window)")
foreach ($n in $names) {
    $stats = $times[$n] | Measure-Object -Minimum -Maximum
    $lines += '{0,-6} {1,7:N1} ms  (min {2:N1}, max {3:N1})' -f $n, (Get-Median $times[$n]), $stats.Minimum, $stats.Maximum
}
$ratio = (Get-Median $times.shell) / (Get-Median $times.bare)
$ok = $ratio -le 3
$lines += 'ratio {0:N2}  {1}' -f $ratio, (Format-Verdict $ok)
Publish 'U4 startup' $lines $ok
if (-not $ok) { $failed++ }

# U6, U7 idle --------------------------------------------------------------------------------------
$idle = @{}
foreach ($n in $names) {
    $idle[$n] = Invoke-Probe $n 'idle'
    Start-Sleep -Milliseconds 300
}
$shell = $idle.shell
$quiet = $shell.timers -eq 0 -and $shell.paints -eq 0
$small = $shell.working_set_kib -le 40 * 1024
$lines = @("idle at ${Size}: 3 s to settle after the first frame, then 5 s untouched (gates: U6 no timer and no paint; U7 working set <= 40 MiB)")
foreach ($n in $names) {
    $r = $idle[$n]
    $lines += '{0,-6} CPU {1,6:N1} ms  messages {2}  timers {3}  paints {4}  working set {5,5:N1} MiB  private {6,5:N1} MiB' -f `
        $n, ($r.idle_cpu_us / 1000), $r.messages, $r.timers, $r.paints, ($r.working_set_kib / 1024), ($r.private_kib / 1024)
}
$lines += 'U6 {0}  U7 {1}' -f (Format-Verdict $quiet), (Format-Verdict $small)
# CPU time is charged in clock ticks (15.6 ms), so one tick can be noise.
if ($shell.idle_cpu_us -gt [Math]::Max($idle.bare.idle_cpu_us, 15625)) {
    $lines += 'advisory: the shell used more CPU while idle than bare_window and one clock tick'
}
Publish 'U6, U7 idle' $lines ($quiet -and $small)
if (-not ($quiet -and $small)) { $failed++ }

exit $failed
