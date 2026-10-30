<#
Diagnoses the T3 offline smoke failure (run 41134716846): against the servercore base layer,
RegLoadAppKeyW fails with ERROR_BADDB and DismOpenSession with 0x80070032, while spike S-02 saw
DISM.exe service the same kind of directory. Hypothesis: the layer's hives are dirty (log replay
pending), a read-only app-hive load can't recover them, and DISM.exe recovers them as a side effect.

Order matters: hive state → app-hive loads on copies → dismpro-cli → DISM.exe → hive state →
dismpro-cli again. Everything is reported in one annotation. Throwaway (branch diag/t3-hives).
#>
param(
    [Parameter(Mandatory)] [string] $Cli,
    [Parameter(Mandatory)] [string] $Image
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Cli = (Resolve-Path $Cli).Path
$Image = (Resolve-Path $Image).Path
$config = Join-Path $Image 'Windows\System32\config'
$lines = [Collections.Generic.List[string]]::new()
function Note([string] $Text) { Write-Host $Text; $lines.Add($Text) }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AppHive {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegLoadAppKeyW(string file, out IntPtr key, int sam, int options, int reserved);
    [DllImport("advapi32.dll")] static extern int RegCloseKey(IntPtr key);
    // Returns the Win32 status of loading `file` as an app hive with `sam` access.
    public static int Load(string file, int sam) {
        IntPtr key;
        int status = RegLoadAppKeyW(file, out key, sam, 0, 0);
        if (status == 0) RegCloseKey(key);
        return status;
    }
}
'@

# regf base block: sequence numbers at 4 and 8 differ while a write is incomplete (dirty).
function Get-HiveState([string] $Path) {
    if (-not (Test-Path $Path)) { return "$(Split-Path $Path -Leaf): missing" }
    $b = [byte[]]::new(512)
    $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try { $null = $fs.Read($b, 0, 512) } finally { $fs.Dispose() }
    $sig = [Text.Encoding]::ASCII.GetString($b, 0, 4)
    $s1 = [BitConverter]::ToUInt32($b, 4)
    $s2 = [BitConverter]::ToUInt32($b, 8)
    $logs = foreach ($ext in 'LOG', 'LOG1', 'LOG2') {
        $l = "$Path.$ext"
        if (Test-Path $l) { "$ext $((Get-Item $l -Force).Length) B" }
    }
    $state = if ($s1 -eq $s2) { 'clean' } else { 'DIRTY' }
    "$(Split-Path $Path -Leaf): $((Get-Item $Path -Force).Length) B, sig $sig, seq $s1/$s2 $state; logs: $(if ($logs) { $logs -join ', ' } else { 'none' })"
}

function Get-Tail([string] $Text, [int] $Count = 2) {
    ($Text -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last $Count) -join ' / '
}

function Invoke-Cli([string[]] $Arguments) {
    $out = & $Cli @Arguments --target $Image --json 2>&1 | Out-String
    "dismpro-cli $($Arguments -join ' '): exit $LASTEXITCODE $(if ($LASTEXITCODE) { Get-Tail $out 1 })"
}

function Get-Version([string] $Path) {
    if (Test-Path $Path) { (Get-Item $Path).VersionInfo.FileVersionRaw.ToString() } else { 'missing' }
}

$hives = 'SYSTEM', 'SOFTWARE', 'COMPONENTS', 'DRIVERS', 'DEFAULT'
try {
    $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    Note "host: build $($cv.CurrentBuild).$($cv.UBR); DismApi $(Get-Version "$env:SystemRoot\System32\dismapi.dll"), DismCore $(Get-Version "$env:SystemRoot\System32\Dism\DismCore.dll")"
    Note "image: ntoskrnl $(Get-Version "$Image\Windows\System32\ntoskrnl.exe"), DismCore $(Get-Version "$Image\Windows\System32\Dism\DismCore.dll")"
    Note ''
    Note '### 1. Hives as extracted'
    foreach ($h in $hives) { Note "- $(Get-HiveState (Join-Path $config $h))" }

    Note ''
    Note '### 2. RegLoadAppKeyW on copies (hive + its logs), read vs write access'
    foreach ($h in 'SYSTEM', 'SOFTWARE') {
        foreach ($sam in @(@{ Name = 'KEY_READ'; Value = 0x20019 }, @{ Name = 'KEY_ALL_ACCESS'; Value = 0xF003F })) {
            $dir = Join-Path $env:RUNNER_TEMP "apphive-$h-$($sam.Name)"
            $null = New-Item -ItemType Directory -Force $dir
            Get-ChildItem $config -Force -File | Where-Object { $_.Name -eq $h -or $_.Name -like "$h.LOG*" } |
                Copy-Item -Destination $dir
            $copy = Join-Path $dir $h
            $status = [AppHive]::Load($copy, $sam.Value)
            Note "- $h, $($sam.Name): status $status ($([ComponentModel.Win32Exception]::new($status).Message)); after: $(Get-HiveState $copy)"
        }
    }

    Note ''
    Note '### 3. dismpro-cli before DISM.exe'
    Note "- $(Invoke-Cli 'reboot-reasons')"
    Note "- $(Invoke-Cli 'packages', 'list')"

    Note ''
    Note '### 4. DISM.exe /Get-Packages'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = dism.exe /English "/Image:$Image" /Get-Packages /Format:Table 2>&1 | Out-String
    $count = @($out -split "`n" | Where-Object { $_ -match '~' }).Count
    Note "- exit $LASTEXITCODE in $([math]::Round($sw.Elapsed.TotalSeconds, 1)) s, $count packages $(if ($LASTEXITCODE) { Get-Tail $out 3 })"
    foreach ($h in $hives) { Note "- after: $(Get-HiveState (Join-Path $config $h))" }

    Note ''
    Note '### 5. dismpro-cli after DISM.exe'
    Note "- $(Invoke-Cli 'reboot-reasons')"
    Note "- $(Invoke-Cli 'packages', 'list')"
} catch {
    Note "ERROR (script line $($_.InvocationInfo.ScriptLineNumber)): $($_.Exception.Message)"
}

$text = $lines -join "`n"
"## T3 diagnosis`n`n$text`n" | Add-Content $env:GITHUB_STEP_SUMMARY
Write-Host "::notice title=T3 diagnosis::$($text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A'))"
