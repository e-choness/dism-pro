<#
Builds the T3 test image (roadmap 1.5): the Windows tree in the base layer of Microsoft's servercore
image, as an offline Windows directory that DISM can service. Nothing is downloaded beyond the
docker pull and nothing is redistributed: the directory lives only on the throwaway runner VM.

Spike S-02 (windows-2025 runner, Docker 29.7, process isolation) found:
- `docker export` doesn't support Windows containers, but `docker save` does; the bottom layer of
  the saved image holds the whole OS under Files/, registry hives included.
- DISM.exe services that directory on the VM (packages, features, enable/disable, CheckHealth), but
  not from inside a container: no wimmount.sys, and DISM can't create its log there (error 5).
#>
param(
    [Parameter(Mandatory)] [string] $Out,
    [string] $Image = 'mcr.microsoft.com/windows/servercore:ltsc2025'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
# Windows' own bsdtar: Git's GNU tar on PATH reads "C:\..." as a remote host.
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'
$hive = 'Windows\System32\config\SYSTEM'

# Runs a native command block; throws on a non-zero exit, logs the time taken.
function Invoke-Step([string] $Name, [scriptblock] $Body) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $Body
    if ($LASTEXITCODE) { throw "$Name failed: exit $LASTEXITCODE" }
    Write-Host "${Name}: $([math]::Round($sw.Elapsed.TotalSeconds)) s"
}

if (Test-Path (Join-Path $Out $hive)) {
    Write-Host "offline image already at $Out"
    exit 0
}
$work = "$Out.work"
$null = New-Item -ItemType Directory -Force $work
$saveTar = Join-Path $work 'save.tar'
$saved = Join-Path $work 'saved'
$layer = Join-Path $work 'layer'
Invoke-Step "docker pull $Image" { docker pull -q $Image | Out-Host }
Invoke-Step 'docker save' { docker save $Image -o $saveTar | Out-Host }
$null = New-Item -ItemType Directory -Force $saved, $layer
Invoke-Step 'unpack the saved image' { & $tar -xf $saveTar -C $saved }
Remove-Item $saveTar
# manifest.json lists the layers bottom first.
$base = (Get-Content (Join-Path $saved 'manifest.json') -Raw | ConvertFrom-Json)[0].Layers[0]
Invoke-Step 'extract the base layer' { & $tar -xf (Join-Path $saved $base) -C $layer }
$root = Join-Path $layer 'Files'
if (-not (Test-Path (Join-Path $root $hive))) {
    throw "the base layer of $Image has no Files\$hive"
}
Move-Item $root $Out
Remove-Item $work -Recurse -Force
Write-Host "offline image ready at $Out"
