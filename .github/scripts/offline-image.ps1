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

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

# A failure becomes an annotation with where it happened and the disk space left: the job log needs
# a token to read, and an image too big for the runner's disk looks like any other failure.
trap {
    $at = $_.InvocationInfo
    $drive = Get-PSDrive ([IO.Path]::GetFullPath($Out).Substring(0, 1))
    $text = "line $($at.ScriptLineNumber): $("$($at.Line)".Trim())`n$($_.Exception.Message)`n" +
        ('{0:N1} GiB free on {1}:' -f ($drive.Free / 1GB), $drive.Name)
    Write-Host "::error title=T3 offline image::$(Format-Annotation $text)"
    exit 1
}

# Runs a native command block; throws on a non-zero exit with the last lines it printed, and logs
# the time taken.
function Invoke-Step([string] $Name, [scriptblock] $Body) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $output = @(& $Body 2>&1 | ForEach-Object { "$_" })
    $output | Out-Host
    if ($LASTEXITCODE) {
        throw "$Name failed: exit $LASTEXITCODE$(($output | Select-Object -Last 5 | ForEach-Object { "`n  $_" }) -join '')"
    }
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
Invoke-Step "docker pull $Image" { docker pull -q $Image }
# Which image this run tested: the tag moves with every servicing update.
Write-Host "::notice title=T3 offline image::$Image, $(docker image inspect --format '{{.Id}} created {{.Created}}' $Image)"
Invoke-Step 'docker save' { docker save $Image -o $saveTar }
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
