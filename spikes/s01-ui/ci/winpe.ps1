<#
Spike S-01 (roadmap 1.6, ADR-0002), the WinPE gate: would each exe load in base WinPE (amd64)?

There's no WinPE VM (ADR-0002), so the loader's work is redone statically against the ADK's
winpe.wim, mounted read-only. Every DLL an exe imports must be one of:
  - a file in the image's System32;
  - an API set (api-ms-*, ext-ms-*) that the image's own schema (System32\apisetschema.dll) maps to
    a host DLL that is in System32;
  - comctl32.dll, side by side: the common-controls v6 assembly in the image's WinSxS, which the
    loader uses because every S-01 exe's manifest declares that dependency (gui.manifest).
Also reports the DLLs option A loads at run time only when present (it falls back without them),
and the image's fonts (option A's text needs one of its fallback families; B uses system fonts).

Runs after .github/scripts/adk.ps1 -WinPE has installed the ADK and its WinPE add-on.
Results: an annotation and the job summary; the detail in -Out.
#>
param(
    [string] $Imports = 'spikes/s01-ui/out/c1/imports.txt',
    [string] $Bin = 'spikes/s01-ui/out/bin',
    [string] $Out = 'spikes/s01-ui/out/winpe'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$null = New-Item -ItemType Directory -Force $Out

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

function Publish([string] $Title, [string] $Text) {
    Write-Host "== $Title`n$Text"
    $Text | Set-Content (Join-Path $Out 'summary.txt')
    if ($env:GITHUB_STEP_SUMMARY) { "### $Title`n``````n$Text`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append }
    Write-Host "::notice title=$Title::$(Format-Annotation $Text)"
}

# The API set schema (version 6, Windows 10 and later): the .apiset section of apisetschema.dll.
# Layout per the loader's API_SET_NAMESPACE / _ENTRY / _VALUE_ENTRY structures; every offset is from
# the start of the section. Returns hashed name (up to the last hyphen) -> default host DLL.
function Read-ApiSetSchema([string] $Path) {
    $b = [IO.File]::ReadAllBytes($Path)
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    $count = [BitConverter]::ToUInt16($b, $pe + 6)
    $header = $pe + 24 + [BitConverter]::ToUInt16($b, $pe + 20)
    $base = $null
    for ($i = 0; $i -lt $count; $i++) {
        $s = $header + 40 * $i
        if ([Text.Encoding]::ASCII.GetString($b, $s, 8).TrimEnd([char] 0) -eq '.apiset') {
            $base = [BitConverter]::ToInt32($b, $s + 20) # PointerToRawData
        }
    }
    if ($null -eq $base) { throw "no .apiset section in $Path" }
    $u32 = { param([int] $o) [BitConverter]::ToUInt32($b, $base + $o) }
    $str = { param([int] $o, [int] $len) [Text.Encoding]::Unicode.GetString($b, $base + $o, $len) }
    $version = & $u32 0
    if ($version -ne 6) { throw "API set schema version $version in $Path (this reads version 6)" }
    $map = @{}
    $entries = & $u32 16
    for ($i = 0; $i -lt (& $u32 12); $i++) {
        $e = $entries + 24 * $i
        $name = (& $str (& $u32 ($e + 4)) (& $u32 ($e + 12))).ToLowerInvariant()
        $values = & $u32 ($e + 16)
        $hostName = ''
        for ($j = 0; $j -lt (& $u32 ($e + 20)); $j++) {
            $v = $values + 20 * $j
            # The default host is the value without an importing module name.
            if ((& $u32 ($v + 8)) -eq 0) { $hostName = & $str (& $u32 ($v + 12)) (& $u32 ($v + 16)) }
        }
        $map[$name] = $hostName.ToLowerInvariant()
    }
    $map
}

# What c1 recorded: "<exe>" -> imported DLL names.
$imported = [ordered]@{}
$exe = $null
foreach ($line in Get-Content $Imports) {
    if ($line -match '^pe-compat: (ok|FAIL) +(\S+)') { $exe = Split-Path $Matches[2] -Leaf }
    elseif ($line -match '^\s*all imports: (.*)$' -and $exe) {
        $imported[$exe] = @($Matches[1] -split ',\s*' | ForEach-Object { ($_ -replace ' \(delay-load\)$', '').ToLowerInvariant() })
    }
}
if ($imported.Count -ne 3) { throw "expected the imports of 3 exes in $Imports, found $($imported.Count)" }

$kits = "${env:ProgramFiles(x86)}\Windows Kits\10"
$image = Join-Path $kits 'Assessment and Deployment Kit\Windows Preinstallation Environment\amd64\en-us\winpe.wim'
if (-not (Test-Path $image)) { throw "$image not found (run adk.ps1 -WinPE first)" }
$mount = Join-Path $env:RUNNER_TEMP 's01-winpe'
$null = New-Item -ItemType Directory -Force $mount
dism /English /Mount-Image "/ImageFile:$image" /Index:1 "/MountDir:$mount" /ReadOnly | Out-Host
if ($LASTEXITCODE) { throw "mounting $image failed: $LASTEXITCODE" }
$lines = @()
try {
    $system32 = Join-Path $mount 'Windows\System32'
    $dlls = @{}
    Get-ChildItem $system32 -Filter *.dll -File | ForEach-Object { $dlls[$_.Name.ToLowerInvariant()] = $true }
    $schema = Read-ApiSetSchema (Join-Path $system32 'apisetschema.dll')
    $sxs = @(Get-ChildItem (Join-Path $mount 'Windows\WinSxS') -Directory -Filter 'amd64_microsoft.windows.common-controls_6595b64144ccf1df_6.*' |
            Where-Object { Test-Path (Join-Path $_.FullName 'comctl32.dll') })
    $lines += "base WinPE amd64 ($image): $($dlls.Count) DLLs in System32, $($schema.Count) API sets in its schema"
    $lines += "common-controls v6 side by side: $(if ($sxs) { ($sxs.Name | ForEach-Object { $_ -replace '^amd64_microsoft\.windows\.common-controls_6595b64144ccf1df_', '' }) -join ', ' } else { 'none' })"
    $lines += "comctl32.dll in System32: $([bool] $dlls['comctl32.dll'])"
    $lines += ''

    foreach ($name in $imported.Keys) {
        $bytes = [IO.File]::ReadAllBytes((Join-Path $Bin $name))
        $manifest = [Text.Encoding]::UTF8.GetString($bytes) -match 'Microsoft\.Windows\.Common-Controls'
        $unresolved = @()
        $notes = @()
        foreach ($dll in $imported[$name]) {
            if ($dll -match '^(api|ext)-ms-') {
                $key = ($dll -replace '\.dll$', '') -replace '-\d+$', ''
                $hostDll = $schema[$key]
                if ($hostDll -and $dlls[$hostDll]) { $notes += "$dll -> $hostDll" }
                else { $unresolved += "$dll (schema host: $(if ($hostDll) { "$hostDll, not in System32" } else { 'none' }))" }
            } elseif ($dlls[$dll]) {
                continue
            } elseif ($dll -eq 'comctl32.dll' -and $manifest -and $sxs) {
                $notes += 'comctl32.dll -> WinSxS common-controls v6 (manifest dependency)'
            } else {
                $unresolved += $dll
            }
        }
        $verdict = if ($unresolved) { 'FAIL' } else { 'PASS' }
        $lines += "${name}: $verdict, $($imported[$name].Count) imports$(if ($notes) { '; ' + ($notes -join '; ') })"
        foreach ($u in $unresolved) { $lines += "    not in base WinPE: $u" }
    }

    $optional = 'd3d11.dll', 'dxgi.dll', 'dcomp.dll', 'd2d1.dll'
    $lines += ''
    $lines += "s01-a run-time only (GDI + solid materials without them): $(($optional | ForEach-Object { "$_ $(if ($dlls[$_]) { 'present' } else { 'absent' })" }) -join ', ')"

    $fonts = @(Get-ChildItem (Join-Path $mount 'Windows\Fonts') -File | ForEach-Object { $_.Name.ToLowerInvariant() } | Sort-Object)
    $wanted = [ordered]@{
        'Segoe UI' = 'segoeui.ttf'; 'Tahoma' = 'tahoma.ttf'; 'Arial' = 'arial.ttf'
        'Cascadia Mono' = 'cascadiamono.ttf'; 'Consolas' = 'consola.ttf'; 'Courier New' = 'cour.ttf'
    }
    $lines += "fonts ($($fonts.Count) files): $(($wanted.Keys | ForEach-Object { "$_ $(if ($fonts -contains $wanted[$_]) { 'yes' } else { 'no' })" }) -join ', ')"
    $lines += "  all: $($fonts -join ', ')"
} finally {
    dism /English /Unmount-Image "/MountDir:$mount" /Discard | Out-Host
}
Publish 'S-01 WinPE gate' ($lines -join "`n")
