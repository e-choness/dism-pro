# Which servicing-state stamps move when a package or feature changes, and which stay put across
# read-only queries and idle time (TD-0013 §7.1). A cached package list needs a stamp that moves
# on every change and on nothing else, or it either serves stale results or never hits.
# Advisory, for the throwaway runner VM only: it changes a feature and undoes it with the CLI.
param(
    [Parameter(Mandatory)] [string] $Cli,
    [string] $Feature = 'TelnetClient',
    [int] $IdleSeconds = 30
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Cli = (Resolve-Path $Cli).Path
$dism = Join-Path $env:SystemRoot 'System32\dism.exe'
$cbs = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32;
using Microsoft.Win32.SafeHandles;

public static class KeyStamp {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegQueryInfoKeyW(SafeRegistryHandle key, IntPtr cls, IntPtr clsLen,
        IntPtr reserved, out uint subKeys, IntPtr maxSubKeyLen, IntPtr maxClassLen, out uint values,
        IntPtr maxValueNameLen, IntPtr maxValueLen, IntPtr securityLen, out long lastWrite);

    // Last-write time (FILETIME), subkey and value counts of an open key.
    public static long Info(RegistryKey key, out uint subKeys, out uint values) {
        long lastWrite;
        int rc = RegQueryInfoKeyW(key.Handle, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, out subKeys,
            IntPtr.Zero, IntPtr.Zero, out values, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, out lastWrite);
        if (rc != 0) throw new System.ComponentModel.Win32Exception(rc);
        return lastWrite;
    }

    // The newest last-write time at or below `key`, `depth` levels down; counts the keys read.
    public static long Newest(RegistryKey key, int depth, ref int visited) {
        uint subKeys, values;
        long newest = Info(key, out subKeys, out values);
        visited++;
        if (depth == 0) return newest;
        foreach (string name in key.GetSubKeyNames()) {
            RegistryKey sub;
            try { sub = key.OpenSubKey(name); }
            catch (System.Security.SecurityException) { continue; }
            catch (UnauthorizedAccessException) { continue; }
            if (sub == null) continue;
            using (sub) {
                long t = Newest(sub, depth - 1, ref visited);
                if (t > newest) newest = t;
            }
        }
        return newest;
    }
}
'@

function Format-Time([long] $FileTime) { [DateTime]::FromFileTimeUtc($FileTime).ToString('HH:mm:ss.fffffff') }

function Get-KeyStamp([string] $Path) {
    $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($Path)
    if (-not $key) { return 'absent' }
    try {
        $sub = [uint32] 0; $val = [uint32] 0
        $t = [KeyStamp]::Info($key, [ref] $sub, [ref] $val)
        "$(Format-Time $t) keys=$sub values=$val"
    } finally { $key.Dispose() }
}

function Get-FileStamp([string] $Path) {
    $i = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if (-not $i) { return 'absent' }
    if ($i.PSIsContainer) {
        $n = @(Get-ChildItem -LiteralPath $Path -Force -ErrorAction SilentlyContinue).Count
        return "$($i.LastWriteTimeUtc.ToString('HH:mm:ss.fffffff')) entries=$n"
    }
    "$($i.LastWriteTimeUtc.ToString('HH:mm:ss.fffffff')) $($i.Length) B"
}

# Every candidate, read now. Values are strings; a candidate "moves" when its string changes.
function Get-Stamps {
    $s = [ordered]@{}
    $s['file servicing\Sessions\Sessions.xml'] = Get-FileStamp "$env:SystemRoot\servicing\Sessions\Sessions.xml"
    $s['dir servicing\Sessions'] = Get-FileStamp "$env:SystemRoot\servicing\Sessions"
    $s['dir servicing\Packages'] = Get-FileStamp "$env:SystemRoot\servicing\Packages"
    $s['file WinSxS\pending.xml'] = Get-FileStamp "$env:SystemRoot\WinSxS\pending.xml"
    $s['file Logs\CBS\CBS.log (control)'] = Get-FileStamp "$env:SystemRoot\Logs\CBS\CBS.log"
    foreach ($k in '', 'Packages', 'PackageIndex', 'PackageDetect', 'ComponentDetect', 'SessionsPending',
        'PackagesPending', 'RebootPending', 'Version', 'Notifications', 'ApplicabilityEvaluationCache') {
        $path = if ($k) { "$cbs\$k" } else { $cbs }
        $s["key CBS$(if ($k) { "\$k" })"] = Get-KeyStamp $path
    }
    $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($cbs)
    try {
        foreach ($name in $key.GetValueNames()) { $s["value CBS\$name"] = "$($key.GetValue($name))" }
        $packages = $key.OpenSubKey('Packages')
        $visited = 0
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $t = [KeyStamp]::Newest($packages, 2, [ref] $visited)
        $script:scanMs = $sw.Elapsed.TotalMilliseconds
        $script:scanKeys = $visited
        $packages.Dispose()
        $s['newest key under CBS\Packages (2 levels)'] = Format-Time $t
    } finally { $key.Dispose() }
    $components = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('COMPONENTS')
    if ($components) {
        try {
            $s['key COMPONENTS (when loaded)'] = Get-KeyStamp 'COMPONENTS'
            foreach ($name in $components.GetValueNames()) {
                $v = $components.GetValue($name)
                $s["value COMPONENTS\$name"] = if ($v -is [byte[]]) { [Convert]::ToHexString($v) } else { "$v" }
            }
        } finally { $components.Dispose() }
    } else { $s['key COMPONENTS (when loaded)'] = 'not loaded' }
    $s
}

# Runs a CLI job (plan + apply) and returns its job.v1 document.
function Invoke-Job([string[]] $Arguments) {
    $docs = @(& $Cli @Arguments --yes --json | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
    if ($LASTEXITCODE) { throw "dismpro-cli $($Arguments -join ' ') exited $LASTEXITCODE" }
    $job = $docs | Where-Object schema -EQ 'job.v1' | Select-Object -First 1
    if (-not $job -or $job.status -ne 'completed') { throw "dismpro-cli $($Arguments -join ' '): $($job | ConvertTo-Json -Compress)" }
    $job
}

function Read-Quietly([string] $Exe, [string[]] $Arguments) {
    $null = & $Exe @Arguments
    if ($LASTEXITCODE) { throw "$Exe $($Arguments -join ' ') exited $LASTEXITCODE" }
}

# JSON Lines: one document per line, parsed one at a time.
$features = (& $Cli features list --json | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } |
    Where-Object schema -EQ 'features.v1').features
$initial = [bool] ($features | Where-Object name -EQ $Feature).enabled
$verb = if ($initial) { 'disable' } else { 'enable' }

# Each step's kind says what an ideal stamp does: 'change' moves it, 'read' and 'idle' don't.
$steps = @(
    @{ Name = 'packages list (ours)'; Kind = 'read'; Run = { Read-Quietly $Cli 'packages', 'list', '--json' } }
    @{ Name = 'dism /Get-Packages'; Kind = 'read'; Run = { Read-Quietly $dism '/Online', '/Get-Packages', '/English' } }
    @{ Name = 'features list (ours)'; Kind = 'read'; Run = { Read-Quietly $Cli 'features', 'list', '--json' } }
    @{ Name = "idle $IdleSeconds s"; Kind = 'idle'; Run = { Start-Sleep -Seconds $IdleSeconds } }
    @{ Name = "features $verb $Feature"; Kind = 'change'; Run = { $script:job = Invoke-Job 'features', $verb, $Feature } }
    @{ Name = 'packages list after it'; Kind = 'read'; Run = { Read-Quietly $Cli 'packages', 'list', '--json' } }
    @{ Name = 'journal undo'; Kind = 'change'; Run = { $null = Invoke-Job 'journal', 'undo', $script:job.job } }
    @{ Name = 'dism /Get-Features'; Kind = 'read'; Run = { Read-Quietly $dism '/Online', '/Get-Features', '/English' } }
    @{ Name = "idle $IdleSeconds s again"; Kind = 'idle'; Run = { Start-Sleep -Seconds $IdleSeconds } }
)

$sw = [Diagnostics.Stopwatch]::StartNew()
$before = Get-Stamps
$readMs = $sw.Elapsed.TotalMilliseconds
$moves = [ordered]@{}
foreach ($name in $before.Keys) { $moves[$name] = [Collections.Generic.List[string]]::new() }
$log = [Collections.Generic.List[string]]::new()
foreach ($step in $steps) {
    $t = [Diagnostics.Stopwatch]::StartNew()
    & $step.Run
    $took = $t.Elapsed.TotalSeconds
    $after = Get-Stamps
    $moved = @()
    foreach ($name in (@($before.Keys) + @($after.Keys) | Select-Object -Unique)) {
        if (-not $moves.Contains($name)) { $moves[$name] = [Collections.Generic.List[string]]::new() }
        $was = $before[$name]; $now = $after[$name]
        if ($was -ne $now) { $moved += $name; $moves[$name].Add($step.Name) }
    }
    $log.Add(("{0} ({1}, {2:N1} s): {3}" -f $step.Name, $step.Kind, $took, $(if ($moved) { $moved -join '; ' } else { 'nothing moved' })))
    $before = $after
}

$changes = @($steps | Where-Object Kind -EQ 'change' | ForEach-Object Name)
$quiet = @($steps | Where-Object Kind -NE 'change' | ForEach-Object Name)
$table = foreach ($name in $moves.Keys) {
    $m = $moves[$name]
    $allChanges = @($changes | Where-Object { $m -contains $_ }).Count -eq $changes.Count
    $noise = @($quiet | Where-Object { $m -contains $_ })
    $verdict = if ($allChanges -and -not $noise) { 'QUALIFIES' }
               elseif ($allChanges) { "moves on reads/idle too ($($noise -join ', '))" }
               elseif ($m.Count) { "misses a change (moved on: $($m -join ', '))" }
               else { 'never moved' }
    '{0}: {1}' -f $name, $verdict
}
$summary = @(
    "Candidates read in $([math]::Round($readMs, 1)) ms; the 2-level scan of CBS\Packages read $scanKeys keys in $([math]::Round($scanMs, 1)) ms (PowerShell + P/Invoke)."
    ''
    'Per step:'
    $log
    ''
    'Per candidate:'
    $table
) -join "`n"
Write-Host $summary
$text = $summary.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
Write-Host "::notice title=servicing stamps (TD-0013 §7.1, advisory)::$text"
