<#
Diagnoses the T3 offline smoke failure (run 41134716846), second pass. The first pass (run
41137875399) ruled out dirty hives: the servercore layer's hives are clean with no logs, yet
RegLoadAppKeyW fails with ERROR_BADDB on copies (read or write access) while DISM.exe services the
image; and dismpro-cli's DismOpenSession fails with 0x80070032 ("requires the latest DISM") while
DISM.exe opens the same path.

Hypotheses tested here:
1. RegLoadAppKeyW refuses hives whose keys don't all share one security descriptor (its docs say
   so), i.e. every real system hive. Test hives saved from HKCU with uniform vs mixed security,
   the image's hives, and the Default profile's NTUSER.DAT (TD-0015 loads other profiles' hives).
2. What can read an offline system hive instead, and does it change the file: the Offline
   Registry Library (offreg.dll), RegLoadKeyW, NtLoadKeyEx with REG_OPEN_READ_ONLY.
3. DismOpenSession: dismpro-cli has no application manifest, so the OS reports itself as
   Windows 8 to it; DISM may then judge a Windows 10+ image newer than the host. Test: the DISM
   PowerShell module (a manifested host), the CLI as built, the CLI with a side-by-side manifest,
   and the DISM log lines each run adds.

Everything runs on copies except section 3, which opens the image. One annotation. Throwaway
(branch diag/t3-hives).
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
$lines = [Collections.Generic.List[string]]::new()
function Note([string] $Text) { Write-Host $Text; $lines.Add($Text) }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class Hive {
    static readonly IntPtr HKLM = new IntPtr(unchecked((int)0x80000002));

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegLoadAppKeyW(string file, out IntPtr key, int sam, int options, int reserved);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegLoadKeyW(IntPtr key, string subKey, string file);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegUnLoadKeyW(IntPtr key, string subKey);
    [DllImport("advapi32.dll")] static extern int RegCloseKey(IntPtr key);

    // The Win32 status of loading `file` as a read-only app hive.
    public static int App(string file) {
        IntPtr key;
        int status = RegLoadAppKeyW(file, out key, 0x20019, 0, 0);
        if (status == 0) RegCloseKey(key);
        return status;
    }

    [DllImport("ntdll.dll")]
    static extern int RtlAdjustPrivilege(int privilege, [MarshalAs(UnmanagedType.U1)] bool enable,
        [MarshalAs(UnmanagedType.U1)] bool thread, [MarshalAs(UnmanagedType.U1)] out bool was);

    // SeBackupPrivilege (17) and SeRestorePrivilege (18), needed to load hives under HKLM.
    public static string Privileges() {
        bool was;
        return RtlAdjustPrivilege(17, true, false, out was) + "/" + RtlAdjustPrivilege(18, true, false, out was);
    }

    public static int Load(string mount, string file) { return RegLoadKeyW(HKLM, mount, file); }
    public static int Unload(string mount) { return RegUnLoadKeyW(HKLM, mount); }

    [StructLayout(LayoutKind.Sequential)]
    struct UnicodeString { public ushort Length; public ushort MaximumLength; public IntPtr Buffer; }
    [StructLayout(LayoutKind.Sequential)]
    struct ObjectAttributes {
        public int Length; public IntPtr Root; public IntPtr Name; public uint Attributes;
        public IntPtr Sd; public IntPtr Qos;
    }
    [DllImport("ntdll.dll")]
    static extern int NtLoadKeyEx(ref ObjectAttributes target, ref ObjectAttributes source, uint flags,
        IntPtr trustClassKey, IntPtr evt, uint access, out IntPtr root, IntPtr ioStatus);
    [DllImport("ntdll.dll")] static extern int NtClose(IntPtr handle);

    // Leaks a few bytes per call; this is a throwaway diagnosis.
    static ObjectAttributes Attributes(string name) {
        UnicodeString u = new UnicodeString {
            Length = (ushort)(name.Length * 2), MaximumLength = (ushort)(name.Length * 2 + 2),
            Buffer = Marshal.StringToHGlobalUni(name)
        };
        IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(UnicodeString)));
        Marshal.StructureToPtr(u, p, false);
        // OBJ_CASE_INSENSITIVE
        return new ObjectAttributes { Length = Marshal.SizeOf(typeof(ObjectAttributes)), Name = p, Attributes = 0x40 };
    }

    // NtLoadKeyEx of `file` at HKLM\<mount> with `flags`; the NTSTATUS.
    public static int NtLoad(string mount, string file, uint flags) {
        ObjectAttributes target = Attributes(@"\Registry\Machine\" + mount);
        ObjectAttributes source = Attributes(@"\??\" + file);
        IntPtr root;
        int status = NtLoadKeyEx(ref target, ref source, flags, IntPtr.Zero, IntPtr.Zero, 0, out root, IntPtr.Zero);
        if (status >= 0 && root != IntPtr.Zero) NtClose(root);
        return status;
    }

    [DllImport("offreg.dll", CharSet = CharSet.Unicode)] static extern int OROpenHive(string path, out IntPtr hive);
    [DllImport("offreg.dll")] static extern int ORCloseHive(IntPtr hive);
    [DllImport("offreg.dll", CharSet = CharSet.Unicode)]
    static extern int ORGetValue(IntPtr key, string subKey, string value, out int type, byte[] data, ref int size);

    // Opens `file` with the Offline Registry Library and reads `subKey`\`value` from it.
    public static string Offreg(string file, string subKey, string value) {
        IntPtr hive;
        int status = OROpenHive(file, out hive);
        if (status != 0) return "OROpenHive " + status;
        try {
            byte[] data = new byte[1024];
            int size = data.Length, type;
            status = ORGetValue(hive, subKey, value, out type, data, ref size);
            if (status != 0) return "ORGetValue " + status;
            if (type == 1 || type == 2) return Encoding.Unicode.GetString(data, 0, size).TrimEnd('\0');
            if (type == 4) return BitConverter.ToUInt32(data, 0).ToString();
            return "type " + type + ", " + size + " B";
        } finally { ORCloseHive(hive); }
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr LoadLibraryExW(string file, IntPtr reserved, int flags);
    [DllImport("kernel32.dll")] static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);

    // Whether `exe` embeds an application manifest (RT_MANIFEST 24, id 1).
    public static bool HasManifest(string exe) {
        // LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE
        IntPtr m = LoadLibraryExW(exe, IntPtr.Zero, 0x2 | 0x20);
        if (m == IntPtr.Zero) return false;
        try { return FindResourceW(m, new IntPtr(1), new IntPtr(24)) != IntPtr.Zero; } finally { FreeLibrary(m); }
    }
}
'@

# regf base block: sequence numbers at 4 and 8 differ while a write is incomplete (dirty).
function Get-HiveState([string] $Path) {
    if (-not (Test-Path $Path)) { return 'missing' }
    $b = [byte[]]::new(512)
    $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try { $null = $fs.Read($b, 0, 512) } finally { $fs.Dispose() }
    $s1 = [BitConverter]::ToUInt32($b, 4)
    $s2 = [BitConverter]::ToUInt32($b, 8)
    $logs = foreach ($ext in 'LOG', 'LOG1', 'LOG2') {
        $l = "$Path.$ext"
        if (Test-Path $l) { "$ext $((Get-Item $l -Force).Length) B" }
    }
    $hash = (Get-FileHash $Path -Algorithm SHA256).Hash.Substring(0, 12)
    "$((Get-Item $Path -Force).Length) B, seq $s1/$s2, sha $hash, logs: $(if ($logs) { $logs -join ', ' } else { 'none' })"
}

function Get-Message([int] $Status) { [ComponentModel.Win32Exception]::new($Status).Message }

function Get-Tail([string] $Text, [int] $Count = 2) {
    ($Text -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last $Count) -join ' / '
}

function Get-Version([string] $Path) {
    if (Test-Path $Path) { (Get-Item $Path).VersionInfo.FileVersionRaw.ToString() } else { 'missing' }
}

# A fresh copy of `Path` in its own folder under $work.
function Copy-Hive([string] $Path, [string] $Name) {
    $dir = Join-Path $work $Name
    $null = New-Item -ItemType Directory -Force $dir
    Copy-Item $Path $dir -Force
    Join-Path $dir (Split-Path $Path -Leaf)
}

# A hive saved from HKCU\Software\dpro-diag\<Name> (a root with one value and a subkey `a`).
function New-TestHive([string] $Name, [switch] $Mixed) {
    $key = "HKCU:\Software\dpro-diag\$Name"
    $null = New-Item -Path "$key\a" -Force
    $null = New-ItemProperty -Path $key -Name v -Value x -Force
    if ($Mixed) {
        # An explicit ACE gives `a` its own security descriptor.
        $acl = Get-Acl "$key\a"
        $rule = [Security.AccessControl.RegistryAccessRule]::new('Everyone', 'ReadKey', 'Allow')
        $acl.AddAccessRule($rule)
        Set-Acl "$key\a" $acl
    }
    $file = Join-Path $work "$Name.hiv"
    Remove-Item $file -ErrorAction SilentlyContinue
    $out = reg.exe save "HKCU\Software\dpro-diag\$Name" $file /y 2>&1 | Out-String
    if ($LASTEXITCODE) { throw "reg save ${Name}: $(Get-Tail $out 1)" }
    $file
}

# The DISM log lines a block adds (errors and version lines), truncated.
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
        $added = $text -split "`r?`n" | Where-Object { $_ -match 'Error|Warning|[Vv]ersion|0x8' } |
            Select-Object -First 14 | ForEach-Object {
                $l = $_ -replace '^\S+ \S+, ', '' -replace '\s+', ' '
                if ($l.Length -gt 260) { $l.Substring(0, 260) + '…' } else { $l }
            }
    }
    $result
    foreach ($l in $added) { "    log: $l" }
}

function Invoke-Cli([string] $Exe, [string[]] $Arguments) {
    $out = & $Exe @Arguments --target $Image --json 2>&1 | Out-String
    "$(Split-Path $Exe -Leaf) $($Arguments -join ' '): exit $LASTEXITCODE $(if ($LASTEXITCODE) { Get-Tail $out 1 } else { "($($out.Length) chars)" })"
}

$cvKey = 'Microsoft\Windows NT\CurrentVersion'
try {
    $cv = Get-ItemProperty "HKLM:\SOFTWARE\$cvKey"
    Note "host: build $($cv.CurrentBuild).$($cv.UBR); DismApi $(Get-Version "$env:SystemRoot\System32\dismapi.dll"); offreg.dll $(Get-Version "$env:SystemRoot\System32\offreg.dll")"
    Note "privileges (RtlAdjustPrivilege SeBackup/SeRestore NTSTATUS): $([Hive]::Privileges())"
    Note ''
    Note '### 1. RegLoadAppKeyW (KEY_READ) vs security descriptors'
    $uniform = New-TestHive 'uniform'
    $mixed = New-TestHive 'mixed' -Mixed
    foreach ($t in @(
            @{ Name = 'saved from HKCU, one security descriptor'; Path = $uniform },
            @{ Name = 'saved from HKCU, subkey with its own ACE'; Path = $mixed },
            @{ Name = 'image DEFAULT'; Path = (Copy-Hive (Join-Path $config 'DEFAULT') 'app-default') },
            @{ Name = 'image DRIVERS'; Path = (Copy-Hive (Join-Path $config 'DRIVERS') 'app-drivers') },
            @{ Name = 'image SYSTEM'; Path = (Copy-Hive (Join-Path $config 'SYSTEM') 'app-system') },
            @{ Name = 'host C:\Users\Default\NTUSER.DAT'; Path = (Copy-Hive 'C:\Users\Default\NTUSER.DAT' 'app-ntuser') })) {
        $s = [Hive]::App($t.Path)
        Note "- $($t.Name): $s ($(Get-Message $s))"
    }
    reg.exe delete 'HKCU\Software\dpro-diag' /f *> $null

    Note ''
    Note '### 2. Reading an offline SOFTWARE/SYSTEM hive another way (fresh copies; file state before → after)'
    foreach ($h in @(@{ Name = 'SOFTWARE'; Key = $cvKey; Value = 'CurrentBuild' }, @{ Name = 'SYSTEM'; Key = 'Select'; Value = 'Current' })) {
        $src = Join-Path $config $h.Name

        $copy = Copy-Hive $src "offreg-$($h.Name)"
        $before = Get-HiveState $copy
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $v = try { [Hive]::Offreg($copy, $h.Key, $h.Value) } catch { "FAILED: $($_.Exception.Message)" }
        Note "- offreg $($h.Name): $($h.Value) = $v in $($sw.ElapsedMilliseconds) ms; $before → $(Get-HiveState $copy)"

        foreach ($m in @(@{ Name = 'RegLoadKeyW'; Flags = $null }, @{ Name = 'NtLoadKeyEx REG_OPEN_READ_ONLY'; Flags = 0x2000 })) {
            $copy = Copy-Hive $src "load-$($h.Name)-$($null -eq $m.Flags)"
            $before = Get-HiveState $copy
            $mount = "dpro-diag-$($h.Name)-$(if ($null -eq $m.Flags) { 'w' } else { 'ro' })"
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $s = if ($null -eq $m.Flags) { [Hive]::Load($mount, $copy) } else { [Hive]::NtLoad($mount, $copy, [uint32]$m.Flags) }
            $loadMs = $sw.ElapsedMilliseconds
            if ($s -eq 0) {
                $q = reg.exe query "HKLM\$mount\$($h.Key)" /v $h.Value 2>&1 | Out-String
                $v = if ($q -match "$($h.Value)\s+REG_\w+\s+(\S+)") { $Matches[1] } else { Get-Tail $q 1 }
                $u = [Hive]::Unload($mount)
                Note "- $($m.Name) $($h.Name): load $loadMs ms, $($h.Value) = $v, unload $u; $before → $(Get-HiveState $copy)"
            } else {
                Note "- $($m.Name) $($h.Name): status $s ($('0x{0:x8}' -f $s)); $before → $(Get-HiveState $copy)"
            }
        }
    }

    Note ''
    Note '### 3. DismOpenSession on the image'
    Note "- dismpro-cli embeds a manifest: $([Hive]::HasManifest($Cli))"
    Invoke-WithDismLog {
        $out = powershell.exe -NoProfile -Command "try { (Get-WindowsPackage -Path '$Image').Count } catch { 'FAILED: ' + `$_.Exception.Message }" 2>&1 | Out-String
        "- DISM PowerShell module, Get-WindowsPackage -Path: $(Get-Tail $out 1)"
    } | ForEach-Object { Note $_ }
    Invoke-WithDismLog { "- $(Invoke-Cli $Cli 'packages', 'list')" } | ForEach-Object { Note $_ }
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
    Note '- with a side-by-side manifest declaring Windows 10/11 support:'
    Invoke-WithDismLog { "  - $(Invoke-Cli $exe 'packages', 'list')" } | ForEach-Object { Note $_ }
} catch {
    Note "ERROR (script line $($_.InvocationInfo.ScriptLineNumber)): $($_.Exception.Message)"
}

$text = $lines -join "`n"
"## T3 diagnosis (2)`n`n$text`n" | Add-Content $env:GITHUB_STEP_SUMMARY
Write-Host "::notice title=T3 diagnosis 2::$($text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A'))"
exit 0
