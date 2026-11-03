<#
T3 cross-check of the offline hive reader (ADR-0008) on the runner VM: every key and value of the
offline image's hives, as read by dpro_engine::regf (the regf_dump example) and by Windows' own
offreg.dll, compared line by line. Then CurrentBuild read from SOFTWARE by both, interleaved,
against the ADR-0008 budget (advisory: one VM, no G1 gate yet).

Reported in the log, in the job summary and as annotations (readable without signing in). Exits 1
if a hive differs or can't be read by either side.
#>
param(
    # regf_dump.exe, built from crates/dpro-engine/examples/regf_dump.rs.
    [Parameter(Mandatory)] [string] $Dump,
    # Offline Windows root, from .github/scripts/offline-image.ps1.
    [Parameter(Mandatory)] [string] $Image
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Dump = (Resolve-Path $Dump).Path
$work = Join-Path ([IO.Path]::GetTempPath()) 'regf-check'
$null = New-Item -ItemType Directory -Force $work

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class Off {
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int OROpenHive(string path, out IntPtr hive);
    [DllImport("offreg.dll")] static extern int ORCloseHive(IntPtr hive);
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int OROpenKey(IntPtr parent, string sub, out IntPtr key);
    [DllImport("offreg.dll")] static extern int ORCloseKey(IntPtr key);
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int OREnumKey(IntPtr key, int index, StringBuilder name, ref int cch, IntPtr cls, IntPtr cchCls, out long lastWrite);
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int OREnumValue(IntPtr key, int index, StringBuilder name, ref int cch, out int type, byte[] data, ref int cb);
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int ORGetValue(IntPtr key, string sub, string value, out int type, byte[] data, ref int cb);
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int ORQueryInfoKey(IntPtr key, IntPtr cls, IntPtr cchCls, out int subkeys, out int maxSub, out int maxCls, out int values, out int maxName, out int maxValue, out int sd, out long lastWrite);

    const int ERROR_MORE_DATA = 234;
    static readonly StringBuilder Name = new StringBuilder(32768);

    static ulong Fnv1a(byte[] data, int len) {
        ulong h = 0xcbf29ce484222325;
        for (int i = 0; i < len; i++) { h ^= data[i]; h = unchecked(h * 0x00000100000001b3UL); }
        return h;
    }

    static void Check(int status, string what) { if (status != 0) throw new Exception(what + ": error " + status); }

    // The same lines as regf_dump: types unsigned, names sorted by UTF-16 code unit.
    static void Walk(StreamWriter w, IntPtr key, string path) {
        int subkeys, maxSub, maxCls, values, maxName, maxValue, sd; long lastWrite;
        Check(ORQueryInfoKey(key, IntPtr.Zero, IntPtr.Zero, out subkeys, out maxSub, out maxCls, out values, out maxName, out maxValue, out sd, out lastWrite), "ORQueryInfoKey " + path);
        w.Write("K\t" + path + "\t" + lastWrite + "\t" + subkeys + "\t" + values + "\n");
        var lines = new List<KeyValuePair<string, string>>();
        byte[] data = new byte[Math.Max(maxValue, 16)];
        for (int i = 0; i < values; i++) {
            int cch = Name.Capacity, type, cb = data.Length;
            int status = OREnumValue(key, i, Name, ref cch, out type, data, ref cb);
            if (status == ERROR_MORE_DATA) { data = new byte[cb + 16]; cch = Name.Capacity; cb = data.Length; status = OREnumValue(key, i, Name, ref cch, out type, data, ref cb); }
            Check(status, "OREnumValue " + path);
            string name = Name.ToString(0, cch);
            lines.Add(new KeyValuePair<string, string>(name, "V\t" + name + "\t" + unchecked((uint)type) + "\t" + cb + "\t" + Fnv1a(data, cb).ToString("x16") + "\n"));
        }
        lines.Sort((a, b) => string.CompareOrdinal(a.Key, b.Key));
        foreach (var l in lines) w.Write(l.Value);
        var names = new List<string>();
        for (int i = 0; i < subkeys; i++) {
            int cch = Name.Capacity; long lw;
            Check(OREnumKey(key, i, Name, ref cch, IntPtr.Zero, IntPtr.Zero, out lw), "OREnumKey " + path);
            names.Add(Name.ToString(0, cch));
        }
        names.Sort(string.CompareOrdinal);
        foreach (var name in names) {
            IntPtr sub;
            Check(OROpenKey(key, name, out sub), "OROpenKey " + path + "\\" + name);
            try { Walk(w, sub, path + "\\" + name); } finally { ORCloseKey(sub); }
        }
    }

    public static void Dump(string hivePath, string outPath) {
        IntPtr hive;
        Check(OROpenHive(hivePath, out hive), "OROpenHive");
        try {
            using (var w = new StreamWriter(outPath, false, new UTF8Encoding(false), 1 << 20)) { Walk(w, hive, ""); }
        } finally { ORCloseHive(hive); }
    }

    // Open included, as regf_dump's `time` does.
    public static long TimeBuild(string hivePath) {
        var sw = Stopwatch.StartNew();
        IntPtr hive;
        Check(OROpenHive(hivePath, out hive), "OROpenHive");
        int type, cb = 64; var data = new byte[64];
        Check(ORGetValue(hive, "Microsoft\\Windows NT\\CurrentVersion", "CurrentBuild", out type, data, ref cb), "ORGetValue");
        long us = sw.ElapsedTicks * 1000000 / Stopwatch.Frequency;
        ORCloseHive(hive);
        return us;
    }

    // Line counts and differences of two dumps, with the first `show` differing pairs.
    public static string Compare(string regf, string off, int show, out long differ) {
        long keys = 0, values = 0; differ = 0;
        var report = new StringBuilder();
        using (var ra = new StreamReader(regf, Encoding.UTF8)) using (var rb = new StreamReader(off, Encoding.UTF8)) {
            while (true) {
                string la = ra.ReadLine(), lb = rb.ReadLine();
                if (la == null && lb == null) break;
                if (la != null && la.StartsWith("K\t")) keys++; else if (la != null) values++;
                if (la != lb) {
                    differ++;
                    if (differ <= show) report.Append("\n  regf:   " + (la ?? "<end>") + "\n  offreg: " + (lb ?? "<end>"));
                }
            }
        }
        return keys + " keys, " + values + " values, " + differ + " lines differ" + report;
    }
}
'@

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

function Publish([string] $Title, [string] $Text, [switch] $Failed) {
    Write-Host "== $Title`n$Text"
    if ($env:GITHUB_STEP_SUMMARY) { "### $Title`n``````n$Text`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append }
    $kind = if ($Failed) { 'error' } else { 'notice' }
    Write-Host "::$kind title=$Title::$(Format-Annotation $Text)"
}

$hives = [ordered]@{}
foreach ($h in 'SOFTWARE', 'SYSTEM', 'DEFAULT', 'DRIVERS', 'COMPONENTS', 'SAM', 'SECURITY') {
    $hives[$h] = Join-Path $Image "Windows\System32\config\$h"
}
$hives['Default NTUSER.DAT'] = Join-Path $Image 'Users\Default\NTUSER.DAT'

$lines = @()
$failures = 0
$a = Join-Path $work 'regf.txt'
$b = Join-Path $work 'offreg.txt'
foreach ($name in $hives.Keys) {
    $path = $hives[$name]
    try {
        if (-not (Test-Path $path)) { throw "no file at $path" }
        $mib = [math]::Round((Get-Item $path -Force).Length / 1MB, 1)
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $out = & $Dump dump $path $a 2>&1 | Out-String
        if ($LASTEXITCODE) { throw "regf_dump exited $LASTEXITCODE`: $($out.Trim())" }
        $regfMs = $sw.ElapsedMilliseconds
        $sw.Restart()
        [Off]::Dump($path, $b)
        $offMs = $sw.ElapsedMilliseconds
        $differ = 0L
        $summary = [Off]::Compare($a, $b, 4, [ref] $differ)
        if ($differ) { $failures++ }
        $lines += "$name ($mib MiB, $($out.Trim())): $summary; regf $regfMs ms (process included), offreg $offMs ms"
    } catch {
        $failures++
        $lines += "$name`: FAILED: $($_.Exception.Message)"
    }
}
Publish "regf vs offreg.dll ($failures failed)" ($lines -join "`n") -Failed:([bool] $failures)

# ADR-0008 budget: CurrentBuild costs no more than offreg.dll's (ratio <= 1.0).
$software = $hives['SOFTWARE']
try {
    $regf = @(); $off = @()
    foreach ($i in 1..7) {
        $out = & $Dump time $software
        if ($LASTEXITCODE) { throw "regf_dump time exited $LASTEXITCODE`: $out" }
        $regf += [long] (($out -split ' ')[0])
        $off += [Off]::TimeBuild($software)
    }
    $regf = $regf | Sort-Object; $off = $off | Sort-Object
    $ratio = [math]::Round($regf[3] / [math]::Max($off[3], 1), 3)
    Publish 'regf CurrentBuild timing' "CurrentBuild from the image's SOFTWARE, open included, median of 7 interleaved (us): regf $($regf[3]) (min $($regf[0]), max $($regf[6])), offreg.dll $($off[3]) (min $($off[0]), max $($off[6])); ratio $ratio (budget <= 1.0, advisory)"
} catch {
    Publish 'regf CurrentBuild timing' "FAILED: $($_.Exception.Message)" -Failed
    $failures++
}
exit [int] [bool] $failures
