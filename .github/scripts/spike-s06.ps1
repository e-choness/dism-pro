<#
Spike S-06 (roadmap 1.9, TD-0014 §7.3 and §12 Q1) on the windows-2025 runner VM. Each experiment
writes its report to the job summary and one annotation (readable without signing in), and exits 0.

  -Experiment Hives   Does Windows reorganize registry hives by itself? Reads the base block ("last
                      reorganized" timestamp, Windows 8.1+) and the free space of the live hives (VSS
                      copies via esentutl) and of the servercore image's hives.
  -Experiment Shell   Do broken context-menu handlers cost measurable time? Times the shell's
                      context-menu build (IShellFolder.GetUIObjectOf + IContextMenu.QueryContextMenu,
                      what a right-click does) for one file with 0, 10 and 50 handlers whose CLSID is
                      unregistered or whose DLL is missing.

Hive layout: the base block field at 0xA8 is the last-reorganized FILETIME, its two low bits are flags
(1 = defragmented, 2 = access history cleared); cells are 4-byte sizes, positive = free. Source: the
public regf format documentation (M. Suhanov, "Windows registry file format specification").
#>
param(
    [Parameter(Mandatory)] [ValidateSet('Hives', 'Shell', 'ShellOnce')] [string] $Experiment,
    # ShellOnce: context-menu builds to time in this (fresh) process.
    [int] $Calls = 10
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$work = 'D:\s06'
$null = New-Item -ItemType Directory -Force $work

function Format-Annotation([string] $Text) { $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A') }

function Publish([string] $Title, [string] $Text) {
    $Text = $Text.TrimEnd()
    Write-Host $Text
    "## S-06 $Title`n`n$Text`n" | Add-Content $env:GITHUB_STEP_SUMMARY
    Write-Host "::notice title=S-06 ${Title}::$(Format-Annotation $Text)"
}

Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class Regf {
    public sealed class Info {
        public DateTime Written; public DateTime? Reorganized; public int ReorgFlags;
        public uint Major, Minor, Seq1, Seq2; public long Bins, Allocated, Free; public int Cells, FreeCells;
        public string Name;
    }

    public static Info Read(string path) {
        byte[] b = File.ReadAllBytes(path);
        if (b.Length < 4096 || Encoding.ASCII.GetString(b, 0, 4) != "regf")
            throw new InvalidDataException(path + ": not a regf hive");
        var i = new Info();
        i.Seq1 = BitConverter.ToUInt32(b, 4);
        i.Seq2 = BitConverter.ToUInt32(b, 8);
        i.Written = DateTime.FromFileTimeUtc(BitConverter.ToInt64(b, 0x0C));
        i.Major = BitConverter.ToUInt32(b, 0x14);
        i.Minor = BitConverter.ToUInt32(b, 0x18);
        long dataSize = BitConverter.ToUInt32(b, 0x28);
        i.Name = Encoding.Unicode.GetString(b, 0x30, 64).TrimEnd('\0');
        long reorg = BitConverter.ToInt64(b, 0xA8);
        i.ReorgFlags = (int)(reorg & 3);
        if ((reorg & ~3L) > 0) i.Reorganized = DateTime.FromFileTimeUtc(reorg & ~3L);
        long off = 4096, end = Math.Min(b.LongLength, 4096 + dataSize);
        while (off + 0x20 <= end && Encoding.ASCII.GetString(b, (int)off, 4) == "hbin") {
            int size = BitConverter.ToInt32(b, (int)off + 8);
            if (size <= 0) break;
            i.Bins += size;
            long c = off + 0x20, cend = Math.Min(off + size, end);
            while (c + 4 <= cend) {
                int cs = BitConverter.ToInt32(b, (int)c);
                if (cs == 0 || cs == int.MinValue) break;
                int len = Math.Abs(cs);
                if (cs > 0) { i.Free += len; i.FreeCells++; } else { i.Allocated += len; }
                i.Cells++;
                c += len;
            }
            off += size;
        }
        return i;
    }
}

public static class ShellMenu {
    [ComImport, Guid("000214E6-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellFolder {
        [PreserveSig] int ParseDisplayName(IntPtr a, IntPtr b, IntPtr c, IntPtr d, IntPtr e, IntPtr f);
        [PreserveSig] int EnumObjects(IntPtr a, int b, IntPtr c);
        [PreserveSig] int BindToObject(IntPtr a, IntPtr b, IntPtr c, IntPtr d);
        [PreserveSig] int BindToStorage(IntPtr a, IntPtr b, IntPtr c, IntPtr d);
        [PreserveSig] int CompareIDs(IntPtr a, IntPtr b, IntPtr c);
        [PreserveSig] int CreateViewObject(IntPtr a, IntPtr b, IntPtr c);
        [PreserveSig] int GetAttributesOf(uint a, IntPtr b, IntPtr c);
        // One child pidl, passed by reference (COM interop would marshal an array as a SAFEARRAY).
        [PreserveSig] int GetUIObjectOf(IntPtr hwnd, uint cidl, ref IntPtr apidl, ref Guid riid, IntPtr reserved, out IntPtr ppv);
    }

    [ComImport, Guid("000214E4-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IContextMenu {
        [PreserveSig] int QueryContextMenu(IntPtr hmenu, uint indexMenu, uint idCmdFirst, uint idCmdLast, uint uFlags);
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern int SHParseDisplayName(string name, IntPtr bindCtx, out IntPtr pidl, uint sfgaoIn, out uint sfgaoOut);
    [DllImport("shell32.dll")]
    static extern int SHBindToParent(IntPtr pidl, ref Guid riid, out IShellFolder folder, out IntPtr last);
    [DllImport("user32.dll")] static extern IntPtr CreatePopupMenu();
    [DllImport("user32.dll")] static extern bool DestroyMenu(IntPtr menu);
    [DllImport("user32.dll")] static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("ole32.dll")] static extern void CoTaskMemFree(IntPtr p);

    // One right-click's worth of work per call, on an STA thread as the shell requires.
    // Returns per-call milliseconds; the last element is the menu item count.
    public static double[] Measure(string file, int calls) {
        double[] result = null;
        Exception error = null;
        var t = new Thread(() => {
            try { result = Run(file, calls); } catch (Exception e) { error = e; }
        });
        t.SetApartmentState(ApartmentState.STA);
        t.Start();
        t.Join();
        if (error != null) throw error;
        return result;
    }

    static double[] Run(string file, int calls) {
        IntPtr pidl; uint attrs;
        Marshal.ThrowExceptionForHR(SHParseDisplayName(file, IntPtr.Zero, out pidl, 0, out attrs));
        var times = new List<double>();
        int items = 0;
        try {
            Guid iidFolder = typeof(IShellFolder).GUID, iidMenu = typeof(IContextMenu).GUID;
            for (int k = 0; k < calls; k++) {
                var sw = Stopwatch.StartNew();
                IShellFolder folder; IntPtr last;
                Marshal.ThrowExceptionForHR(SHBindToParent(pidl, ref iidFolder, out folder, out last));
                IntPtr unk;
                Marshal.ThrowExceptionForHR(folder.GetUIObjectOf(IntPtr.Zero, 1, ref last, ref iidMenu, IntPtr.Zero, out unk));
                var menu = (IContextMenu)Marshal.GetObjectForIUnknown(unk);
                IntPtr h = CreatePopupMenu();
                Marshal.ThrowExceptionForHR(menu.QueryContextMenu(h, 0, 1, 0x7FFF, 0));
                items = GetMenuItemCount(h);
                DestroyMenu(h);
                Marshal.ReleaseComObject(menu);
                Marshal.Release(unk);
                Marshal.ReleaseComObject(folder);
                times.Add(sw.Elapsed.TotalMilliseconds);
            }
        } finally { CoTaskMemFree(pidl); }
        times.Add(items);
        return times.ToArray();
    }
}
'@

function Get-Median([double[]] $Values) {
    $s = $Values | Sort-Object
    if (-not $s) { return 0 }
    $s[[int][math]::Floor($s.Count / 2)]
}

function Format-Hive([string] $Label, [string] $Path, [datetime] $Boot) {
    try {
        $i = [Regf]::Read($Path)
        $reorg = if ($i.Reorganized) {
            # PowerShell unwraps Nullable<DateTime>: no .Value here.
            $age = $Boot - $i.Reorganized
            "$($i.Reorganized.ToString('u')) (flags $($i.ReorgFlags), $([math]::Round($age.TotalDays, 1)) d before boot)"
        } else { 'never (field 0)' }
        $mb = [math]::Round($i.Bins / 1MB, 1)
        $free = if ($i.Bins) { [math]::Round(100 * $i.Free / $i.Bins, 1) } else { 0 }
        "| $Label | $mb MiB | $free % ($($i.FreeCells) free cells) | $reorg | $($i.Written.ToString('u')) | $($i.Major).$($i.Minor) |"
    } catch { "| $Label | FAILED: $($_.Exception.Message) | | | | |" }
}

switch ($Experiment) {
    'Hives' {
        $os = Get-CimInstance Win32_OperatingSystem
        $boot = $os.LastBootUpTime.ToUniversalTime()
        $lines = @("$($os.Caption) $($os.Version); installed $($os.InstallDate.ToUniversalTime().ToString('u')); last boot $($boot.ToString('u'))", '')
        $lines += '| hive | bins | free space | last reorganized | last written | format |', '|---|--:|--:|---|---|---|'
        $copies = Join-Path $work 'live'
        $null = New-Item -ItemType Directory -Force $copies
        $live = [ordered]@{}
        foreach ($h in 'SYSTEM', 'SOFTWARE', 'COMPONENTS', 'DRIVERS', 'SAM', 'SECURITY', 'DEFAULT') {
            $live["config\$h"] = Join-Path $env:SystemRoot "System32\config\$h"
        }
        $live['NTUSER.DAT (runner user)'] = Join-Path $env:USERPROFILE 'NTUSER.DAT'
        $live['UsrClass.dat (runner user)'] = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\UsrClass.dat'
        $notes = @()
        # Each part reports its own failure, so one broken part doesn't hide the others' data.
        function Get-Failure([string] $Part, $Err) {
            "$Part FAILED (script line $($Err.InvocationInfo.ScriptLineNumber)): $($Err.Exception.Message)"
        }
        try {
            $n = 0
            foreach ($k in $live.Keys) {
                $dst = Join-Path $copies "hive$((++$n))"
                # esentutl /vss copies a file the kernel holds open, through a shadow copy.
                $out = esentutl.exe /y $live[$k] /vss /d $dst 2>&1 | Out-String
                if ($LASTEXITCODE -or -not (Test-Path $dst)) {
                    $lines += "| live $k | copy FAILED: $(($out -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last 2) -join ' / ') | | | | |"
                    continue
                }
                $lines += Format-Hive "live $k" $dst $boot
            }
        } catch { $notes += Get-Failure 'live hives' $_ }

        # The servercore image's hives: one more data point, from an image built by Microsoft.
        try {
            $image = 'mcr.microsoft.com/windows/servercore:ltsc2025'
            $tar = Join-Path $env:SystemRoot 'System32\tar.exe'
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $out = docker pull -q $image 2>&1 | Out-String
            if ($LASTEXITCODE) { throw "docker pull: exit $LASTEXITCODE, $($out.Trim())" }
            $saveTar = Join-Path $work 'save.tar'
            $out = docker save $image -o $saveTar 2>&1 | Out-String
            if ($LASTEXITCODE -or -not (Test-Path $saveTar)) { throw "docker save: exit $LASTEXITCODE, $($out.Trim())" }
            $saved = Join-Path $work 'saved'
            $null = New-Item -ItemType Directory -Force $saved
            $out = & $tar -xf $saveTar -C $saved 2>&1 | Out-String
            if ($LASTEXITCODE) { throw "tar (save): exit $LASTEXITCODE, $($out.Trim())" }
            $manifest = Join-Path $saved 'manifest.json'
            if (-not (Test-Path $manifest)) {
                $top = (Get-ChildItem $saved | Select-Object -First 10 -ExpandProperty Name) -join ', '
                throw "no manifest.json in the saved image (top level: $top)"
            }
            $base = (Get-Content $manifest -Raw | ConvertFrom-Json)[0].Layers[0]
            $layer = Join-Path $work 'layer'
            $null = New-Item -ItemType Directory -Force $layer
            $members = 'SYSTEM', 'SOFTWARE', 'COMPONENTS' | ForEach-Object { "Files/Windows/System32/config/$_" }
            $out = & $tar -xf (Join-Path $saved $base) -C $layer @members 2>&1 | Out-String
            if ($LASTEXITCODE) { throw "tar (base layer $base): exit $LASTEXITCODE, $($out.Trim())" }
            foreach ($h in 'SYSTEM', 'SOFTWARE', 'COMPONENTS') {
                $lines += Format-Hive "servercore image config\$h" (Join-Path $layer "Files\Windows\System32\config\$h") $boot
            }
            $notes += "servercore image: pull + save + extract in $([math]::Round($sw.Elapsed.TotalSeconds)) s"
        } catch { $notes += Get-Failure 'servercore image hives' $_ }

        try {
            $cm = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Configuration Manager'
            $props = if (Test-Path $cm) { (Get-ItemProperty $cm).PSObject.Properties } else { @() }
            $values = $props | Where-Object Name -NotLike 'PS*' | ForEach-Object { "$($_.Name)=$($_.Value)" }
            $notes += "Configuration Manager values: $(if ($values) { $values -join ', ' } else { 'none' })"
        } catch { $notes += Get-Failure 'Configuration Manager values' $_ }
        Publish 'hives' (($lines + '' + $notes) -join "`n")
    }
    'Shell' {
        $ext = '.s06test'
        $progId = 's06file'
        $classes = 'HKLM:\SOFTWARE\Classes'
        $file = Join-Path $work "probe$ext"
        Set-Content $file 'x'
        $null = New-Item "$classes\$ext" -Value $progId -Force
        $null = New-Item "$classes\$progId" -Value 'S-06 probe' -Force
        $handlers = "$classes\$progId\shellex\ContextMenuHandlers"
        # Every config gets fresh CLSIDs, so nothing the shell caches by CLSID carries over.
        function Set-Handlers([int] $Count, [string] $Kind) {
            Remove-Item $handlers -Recurse -Force -ErrorAction SilentlyContinue
            $null = New-Item $handlers -Force
            for ($i = 0; $i -lt $Count; $i++) {
                $clsid = "{$([guid]::NewGuid())}"
                $null = New-Item "$handlers\S06Handler$i" -Value $clsid -Force
                if ($Kind -eq 'missing DLL') {
                    $server = New-Item "$classes\CLSID\$clsid\InprocServer32" -Value "C:\s06-missing\handler$i.dll" -Force
                    $null = New-ItemProperty $server.PSPath -Name ThreadingModel -Value Apartment
                }
            }
        }
        $configs = @(
            @{ Count = 0; Kind = 'none' }
            @{ Count = 10; Kind = 'unregistered CLSID' }
            @{ Count = 50; Kind = 'unregistered CLSID' }
            @{ Count = 10; Kind = 'missing DLL' }
            @{ Count = 50; Kind = 'missing DLL' }
        )
        $times = @{}
        $firsts = @{}
        $items = @{}
        $rounds = 5
        $failures = @()
        for ($r = 0; $r -lt $rounds; $r++) {
            for ($k = 0; $k -lt $configs.Count; $k++) {
                $c = $configs[($k + $r) % $configs.Count]
                $key = "$($c.Count) $($c.Kind)"
                Set-Handlers $c.Count $c.Kind
                # A fresh process per measurement: the shell caches association data per process.
                $out = pwsh -NoProfile -NonInteractive -File $PSCommandPath -Experiment ShellOnce -Calls 11 2>&1 | Out-String
                if ($out -notmatch 'TIMES (\S+) ITEMS (\d+)') {
                    $failures += "$key round ${r}: $($out.Trim())"
                    continue
                }
                $m = $Matches[1] -split ',' | ForEach-Object { [double] $_ }
                $firsts[$key] += @($m[0])          # first build in the process: handler lookup uncached
                $times[$key] += @($m[1..($m.Count - 1)])
                $items[$key] = [int] $Matches[2]
            }
        }
        Remove-Item $handlers -Recurse -Force -ErrorAction SilentlyContinue
        $baseKey = '0 none'
        $base = Get-Median $times[$baseKey]
        $baseFirst = Get-Median $firsts[$baseKey]
        $lines = @("$rounds interleaved rounds; each: a fresh process, 1 first build + 10 repeat builds of one file's context menu. Medians in ms.", '')
        $lines += '| handlers | first build | ÷ none | repeat build | ÷ none | p90 repeat | menu items |', '|---|--:|--:|--:|--:|--:|--:|'
        foreach ($c in $configs) {
            $key = "$($c.Count) $($c.Kind)"
            if (-not $times[$key]) { $lines += "| $key | failed | | | | | |"; continue }
            $sorted = $times[$key] | Sort-Object
            $med = Get-Median $times[$key]
            $first = Get-Median $firsts[$key]
            $p90 = $sorted[[int][math]::Floor(0.9 * ($sorted.Count - 1))]
            $ratio = { param($a, $b) if ($b) { [math]::Round($a / $b, 2) } else { '-' } }
            $lines += "| $key | $([math]::Round($first, 2)) | $(& $ratio $first $baseFirst) | $([math]::Round($med, 2)) | $(& $ratio $med $base) | $([math]::Round($p90, 2)) | $($items[$key]) |"
        }
        if ($failures) { $lines += '', 'failures:', ($failures | Select-Object -First 5) }
        Publish 'shell extensions' ($lines -join "`n")
    }
    'ShellOnce' {
        $m = [ShellMenu]::Measure((Join-Path $work 'probe.s06test'), $Calls)
        $t = $m[0..($m.Count - 2)] | ForEach-Object { $_.ToString('0.###', [Globalization.CultureInfo]::InvariantCulture) }
        Write-Output "TIMES $($t -join ',') ITEMS $($m[-1])"
    }
}
exit 0
