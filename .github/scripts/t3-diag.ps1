<#
Diagnoses the T3 offline smoke failure (run 41134716846), third pass. Passes 1 and 2 found:
- the servercore layer's hives are clean; RegLoadAppKeyW fails on its SYSTEM, DRIVERS and DEFAULT
  with ERROR_BADDB, but loads a hive saved from HKCU (with or without mixed security descriptors)
  and C:\Users\Default\NTUSER.DAT, so "one security descriptor per app hive" isn't the cause;
- offreg.dll reads the image's hives without changing them; RegLoadKeyW reads them but rewrites the
  file (sequence numbers, size, logs);
- the DISM PowerShell module opens the image; dismpro-cli's DismOpenSession doesn't (pass 2's
  annotation was cut at ~4 KB before the CLI's results).

This pass: (1) dismpro-cli with and without a side-by-side manifest, with the DISM log lines that
show the host version DISM sees; (2) RegLoadAppKeyW on VSS copies of the runner's live hives (what
an offline installation looks like); (3) the anatomy of each hive (base block, cell kinds, key
flags, layered-key bits, symbolic links) to find what the failing hives have in common. One small
annotation per section. Throwaway (branch diag/t3-hives).
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
$work = Join-Path $env:RUNNER_TEMP 't3-diag'
$null = New-Item -ItemType Directory -Force $work

function Format-Annotation([string] $Text) { $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A') }

function Publish([string] $Title, [string[]] $Lines) {
    $text = ($Lines -join "`n").TrimEnd()
    Write-Host $text
    "## $Title`n`n$text`n" | Add-Content $env:GITHUB_STEP_SUMMARY
    $t = (Format-Annotation $Title).Replace(':', '%3A').Replace(',', '%2C')
    Write-Host "::notice title=$t::$(Format-Annotation $text)"
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;

public static class Hive {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegLoadAppKeyW(string file, out IntPtr key, int sam, int options, int reserved);
    [DllImport("advapi32.dll")] static extern int RegCloseKey(IntPtr key);

    // The Win32 status of loading `file` as a read-only app hive.
    public static int App(string file) {
        IntPtr key;
        int status = RegLoadAppKeyW(file, out key, 0x20019, 0, 0);
        if (status == 0) RegCloseKey(key);
        return status;
    }

    static string Count(SortedDictionary<int, int> d, string format) {
        return d.Count == 0 ? "none" : string.Join(" ", d.Select(p => p.Key.ToString(format) + ":" + p.Value));
    }

    static void Add(SortedDictionary<int, int> d, int key) {
        int n;
        d[key] = d.TryGetValue(key, out n) ? n + 1 : 1;
    }

    // Base block fields and the cells a regf file uses (layout: the public regf specification).
    public static string Anatomy(string path) {
        byte[] f = File.ReadAllBytes(path);
        if (f.Length < 0x1000 || Encoding.ASCII.GetString(f, 0, 4) != "regf") return "not regf";
        uint sum = 0;
        for (int i = 0; i < 0x1FC; i += 4) sum ^= BitConverter.ToUInt32(f, i);
        bool checksum = sum == BitConverter.ToUInt32(f, 0x1FC);
        var nkFlags = new SortedDictionary<int, int>();
        var layer = new SortedDictionary<int, int>();
        var valueTypes = new SortedDictionary<int, int>();
        int nk = 0, vk = 0, sk = 0, db = 0, other = 0;
        long end = Math.Min(f.Length, 0x1000L + BitConverter.ToUInt32(f, 0x28));
        long bin = 0x1000;
        string error = "";
        while (bin + 0x20 <= end) {
            if (Encoding.ASCII.GetString(f, (int)bin, 4) != "hbin") { error = " BAD hbin @" + bin.ToString("x"); break; }
            uint size = BitConverter.ToUInt32(f, (int)bin + 8);
            if (size < 0x1000 || bin + size > f.Length) { error = " BAD hbin size @" + bin.ToString("x"); break; }
            long c = bin + 0x20;
            while (c + 8 <= bin + size) {
                int cs = BitConverter.ToInt32(f, (int)c);
                if (cs == 0) break;
                long len = Math.Abs((long)cs);
                if (cs < 0) {
                    string sig = Encoding.ASCII.GetString(f, (int)c + 4, 2);
                    if (sig == "nk" && c + 4 + 0x4C <= bin + size) {
                        nk++;
                        int flags = BitConverter.ToUInt16(f, (int)c + 6);
                        for (int b = 0; b < 16; b++) if ((flags & (1 << b)) != 0) Add(nkFlags, 1 << b);
                        int bits = f[c + 4 + 0x0D];
                        if (bits != 0) Add(layer, bits);
                    } else if (sig == "vk" && c + 4 + 0x14 <= bin + size) {
                        vk++;
                        int type = BitConverter.ToInt32(f, (int)c + 4 + 0x0C);
                        if (type < 0 || type > 11) Add(valueTypes, type);
                        else if (type == 6) Add(valueTypes, 6);
                    } else if (sig == "sk") sk++;
                    else if (sig == "db") db++;
                    else other++;
                }
                c += len;
            }
            bin += size;
        }
        return string.Format(
            "v{0}.{1} type {2} format {3} flags@0x90 {4:x} boot {5}/{6} checksum {7}; nk {8} vk {9} sk {10} db {11} other {12}; " +
            "nk flags {13}; layered-key byte {14}; unusual value types {15}{16}",
            BitConverter.ToUInt32(f, 0x14), BitConverter.ToUInt32(f, 0x18), BitConverter.ToUInt32(f, 0x1C),
            BitConverter.ToUInt32(f, 0x20), BitConverter.ToUInt32(f, 0x90), BitConverter.ToUInt32(f, 0xFF8),
            BitConverter.ToUInt32(f, 0xFFC), checksum ? "ok" : "BAD", nk, vk, sk, db, other,
            Count(nkFlags, "x"), Count(layer, "x"), Count(valueTypes, "d"), error);
    }
}
'@

function Get-Message([int] $Status) { [ComponentModel.Win32Exception]::new($Status).Message }

function Get-Tail([string] $Text, [int] $Count = 1) {
    ($Text -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last $Count) -join ' / '
}

# A fresh copy of `Path` (and its logs, if any) in its own folder under $work.
function Copy-Hive([string] $Path, [string] $Name) {
    $dir = Join-Path $work $Name
    $null = New-Item -ItemType Directory -Force $dir
    Copy-Item $Path $dir -Force
    Join-Path $dir (Split-Path $Path -Leaf)
}

# A VSS copy of a hive the kernel holds open (plus its logs), via esentutl.
function Copy-LiveHive([string] $Path, [string] $Name) {
    $dir = Join-Path $work $Name
    $null = New-Item -ItemType Directory -Force $dir
    foreach ($suffix in '', '.LOG1', '.LOG2') {
        if (-not (Test-Path "$Path$suffix" -PathType Leaf)) { continue }
        $out = esentutl.exe /y "$Path$suffix" /vss /d (Join-Path $dir ((Split-Path $Path -Leaf) + $suffix)) 2>&1 | Out-String
        if ($LASTEXITCODE) { throw "esentutl $Path${suffix}: $(Get-Tail $out 2)" }
    }
    Join-Path $dir (Split-Path $Path -Leaf)
}

# DISM log lines a block adds: the host/target versions DISM sees and errors.
function Invoke-WithDismLog([scriptblock] $Body) {
    $log = Join-Path $env:SystemRoot 'Logs\DISM\dism.log'
    $start = if (Test-Path $log) { (Get-Item $log).Length } else { 0 }
    $result = & $Body
    $added = @()
    if (Test-Path $log) {
        $fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
        try {
            $null = $fs.Seek($start, 'Begin')
            $text = [IO.StreamReader]::new($fs).ReadToEnd()
        } finally { $fs.Dispose() }
        $added = $text -split "`r?`n" | Where-Object { $_ -match 'Host machine information|Target image information|API Version|^\S+ \S+, Error' } |
            Select-Object -First 6 | ForEach-Object {
                $l = ($_ -replace '^\S+ \S+, ', '' -replace 'PID=\d+ TID=\d+ ', '' -replace '\s+', ' ')
                if ($l.Length -gt 220) { $l.Substring(0, 220) + '…' } else { $l }
            }
    }
    $result
    foreach ($l in $added) { "    - log: $l" }
}

function Invoke-Cli([string] $Exe, [string[]] $Arguments) {
    $out = & $Exe @Arguments --target $Image --json 2>&1 | Out-String
    "$($Arguments -join ' '): exit $LASTEXITCODE $(if ($LASTEXITCODE) { Get-Tail $out } else { "($($out.Length) chars)" })"
}

# 1. DISM
$lines = @()
try {
    $lines += "- as built, packages list:"
    $lines += Invoke-WithDismLog { "  - $(Invoke-Cli $Cli 'packages', 'list')" }
    $sxs = Join-Path $work 'manifested'
    $null = New-Item -ItemType Directory -Force $sxs
    $exe = Join-Path $sxs 'dismpro-cli.exe'
    Copy-Item $Cli $exe
    Set-Content "$exe.manifest" -Encoding utf8 -Value @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
'@
    $lines += "- with a side-by-side manifest (supportedOS Windows 10+), packages list:"
    $lines += Invoke-WithDismLog { "  - $(Invoke-Cli $exe 'packages', 'list')" }
    $lines += "- with the manifest, features list: $(Invoke-Cli $exe 'features', 'list')"
} catch { $lines += "ERROR (line $($_.InvocationInfo.ScriptLineNumber)): $($_.Exception.Message)" }
Publish 'T3 diag 3: DismOpenSession' $lines

# 2. App hives from a live (VSS) installation
$lines = @()
$live = @{}
foreach ($h in 'SYSTEM', 'SOFTWARE', 'DRIVERS', 'DEFAULT', 'COMPONENTS') {
    try {
        $live[$h] = Copy-LiveHive "$env:SystemRoot\System32\config\$h" "live-$h"
        $s = [Hive]::App($live[$h])
        $lines += "- live $h (VSS copy + logs): $s ($(Get-Message $s))"
    } catch { $lines += "- live ${h}: ERROR $($_.Exception.Message)" }
}
Publish 'T3 diag 3: RegLoadAppKeyW on live hives' $lines

# 3. Anatomy
$lines = @()
$samples = [ordered]@{}
foreach ($h in 'SYSTEM', 'SOFTWARE', 'DRIVERS', 'DEFAULT', 'COMPONENTS') { $samples["image $h"] = Join-Path $config $h }
foreach ($h in 'SYSTEM', 'DEFAULT') { if ($live[$h]) { $samples["live $h"] = $live[$h] } }
$samples['Default NTUSER.DAT'] = 'C:\Users\Default\NTUSER.DAT'
foreach ($k in $samples.Keys) {
    try {
        $copy = Copy-Hive $samples[$k] ("anatomy-" + ($k -replace '\W', '-'))
        $lines += "- ${k}: app $([Hive]::App($copy)); $([Hive]::Anatomy($copy))"
    } catch { $lines += "- ${k}: ERROR $($_.Exception.Message)" }
}
Publish 'T3 diag 3: hive anatomy' $lines
exit 0
