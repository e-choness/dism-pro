<#
Throwaway cross-check (diag/t3-regf only, ADR-0008): every key and value of real hives, as read by
the regf reader and by offreg.dll, compared line by line; then CurrentBuild from SOFTWARE by both,
interleaved in this job. Results go to annotations.
#>
param(
    [Parameter(Mandatory)] [string] $Dump,
    [Parameter(Mandatory)] [string] $Image
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$work = Join-Path $env:RUNNER_TEMP 'regf'
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

    static readonly StringBuilder Name = new StringBuilder(32768);

    static ulong Fnv(byte[] data, int len) {
        ulong h = 0xcbf29ce484222325;
        for (int i = 0; i < len; i++) { h ^= data[i]; h = unchecked(h * 0x00000100000001b3UL); }
        return h;
    }

    static void Check(int status, string what) { if (status != 0) throw new Exception(what + ": " + status); }

    static void Walk(StreamWriter w, IntPtr key, string path) {
        int subkeys, maxSub, maxCls, values, maxName, maxValue, sd; long lastWrite;
        Check(ORQueryInfoKey(key, IntPtr.Zero, IntPtr.Zero, out subkeys, out maxSub, out maxCls, out values, out maxName, out maxValue, out sd, out lastWrite), "ORQueryInfoKey " + path);
        w.Write("K\t" + path + "\t" + lastWrite + "\t" + subkeys + "\t" + values + "\n");
        var lines = new List<KeyValuePair<string, string>>();
        byte[] data = new byte[Math.Max(maxValue, 16)];
        for (int i = 0; i < values; i++) {
            int cch = Name.Capacity, type, cb = data.Length;
            int status = OREnumValue(key, i, Name, ref cch, out type, data, ref cb);
            if (status == 234) { data = new byte[cb + 16]; cch = Name.Capacity; cb = data.Length; status = OREnumValue(key, i, Name, ref cch, out type, data, ref cb); }
            Check(status, "OREnumValue " + path);
            string name = Name.ToString(0, cch);
            lines.Add(new KeyValuePair<string, string>(name, "V\t" + name + "\t" + unchecked((uint)type) + "\t" + cb + "\t" + Fnv(data, cb).ToString("x16") + "\n"));
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
            Walk(w, sub, path + "\\" + name);
            ORCloseKey(sub);
        }
    }

    public static void Dump(string hivePath, string outPath) {
        IntPtr hive;
        Check(OROpenHive(hivePath, out hive), "OROpenHive");
        using (var w = new StreamWriter(outPath, false, new UTF8Encoding(false), 1 << 20)) { Walk(w, hive, ""); }
        ORCloseHive(hive);
    }

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

    // Compares two dumps. Returns a summary; lists the first differing lines.
    public static string Compare(string a, string b, int show) {
        long keys = 0, values = 0, differ = 0; var report = new StringBuilder();
        using (var ra = new StreamReader(a, Encoding.UTF8)) using (var rb = new StreamReader(b, Encoding.UTF8)) {
            while (true) {
                string la = ra.ReadLine(), lb = rb.ReadLine();
                if (la == null && lb == null) break;
                if (la != null && la.StartsWith("K\t")) keys++; else if (la != null) values++;
                if (la != lb) {
                    differ++;
                    if (differ <= show) report.Append("  regf:   " + (la ?? "<end>") + "\n  offreg: " + (lb ?? "<end>") + "\n");
                }
            }
        }
        return keys + " keys, " + values + " values, " + differ + " lines differ\n" + report;
    }
}
'@

function Note([string] $Title, [string] $Text) {
    Write-Host "== $Title`n$Text"
    $m = $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
    Write-Host "::notice title=$Title::$m"
}

$saved = @()
foreach ($h in 'SOFTWARE', 'SYSTEM') {
    $file = Join-Path $work "live-$h.hiv"
    reg.exe save "HKLM\$h" $file /y | Out-Null
    $saved += @{ Name = "reg save HKLM\$h"; Path = $file }
}
$hives = @()
foreach ($h in 'SOFTWARE', 'SYSTEM', 'DEFAULT', 'DRIVERS', 'COMPONENTS', 'SAM', 'SECURITY') {
    $hives += @{ Name = "image $h"; Path = Join-Path $Image "Windows\System32\config\$h" }
}
$hives += @{ Name = 'image Default NTUSER.DAT'; Path = Join-Path $Image 'Users\Default\NTUSER.DAT' }
$hives += $saved

$lines = @()
foreach ($h in $hives) {
    if (-not (Test-Path $h.Path)) { $lines += "$($h.Name): no file"; continue }
    $mb = [math]::Round((Get-Item $h.Path -Force).Length / 1MB, 1)
    $a = Join-Path $work 'regf.txt'
    $b = Join-Path $work 'off.txt'
    try {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $d = & $Dump dump $h.Path $a 2>&1 | Out-String
        $code = $LASTEXITCODE
        $regfMs = $sw.ElapsedMilliseconds
        if ($code) { throw "regf_dump exit $code`: $($d.Trim())" }
        $sw.Restart()
        [Off]::Dump($h.Path, $b)
        $offMs = $sw.ElapsedMilliseconds
        $lines += "$($h.Name) ($mb MiB, $($d.Trim())): regf $regfMs ms, offreg $offMs ms: $([Off]::Compare($a, $b, 6))"
    } catch {
        $lines += "$($h.Name) ($mb MiB): FAILED: $($_.Exception.Message)"
    }
}
Note 'regf vs offreg' ($lines -join "`n")

$software = Join-Path $Image 'Windows\System32\config\SOFTWARE'
$regf = @(); $off = @()
foreach ($i in 1..7) {
    $regf += [long](((& $Dump time $software) -split ' ')[0])
    $off += [Off]::TimeBuild($software)
}
$regf = $regf | Sort-Object; $off = $off | Sort-Object
Note 'regf vs offreg timing' "CurrentBuild from SOFTWARE, open included, median of 7 interleaved (us): regf $($regf[3]) (min $($regf[0]) max $($regf[6])), offreg $($off[3]) (min $($off[0]) max $($off[6])); ratio $([math]::Round($regf[3] / $off[3], 3))"
