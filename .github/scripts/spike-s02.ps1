<#
Spike S-02 (roadmap 1.5) experiments on a GitHub windows-2025 runner. One experiment per call; each
writes its observations to the log, the job summary and one annotation, and exits 0 so later
experiments still run. State is shared through a work folder on the roomiest drive.

  E1  runner, Docker engine, isolation mode, cached base images
  E2  export servercore:ltsc2025 and turn it into an offline Windows directory (the test-image source)
  E3  DISM.exe and offline hives against that directory, on the runner
  E4  the same from inside a process-isolated container, plus /Online in a container
  E5  WIM capture and mount, on the runner and inside a container
#>
param([Parameter(Mandatory)] [ValidateSet('E1', 'E2', 'E3', 'E4', 'E5')] [string] $Experiment)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$base = $env:BASE
# Windows' own bsdtar: Git's GNU tar on PATH reads "C:\..." as a remote host.
$tarExe = Join-Path $env:SystemRoot 'System32\tar.exe'
$drive = Get-PSDrive -PSProvider FileSystem | Where-Object Free | Sort-Object Free -Descending | Select-Object -First 1
$work = Join-Path $drive.Root 's02'
$img = Join-Path $work 'img'
$null = New-Item -ItemType Directory -Force $work
$notes = [Collections.Generic.List[string]]::new()

function Note([string] $Text) { $notes.Add($Text); Write-Host $Text }

# Runs a native command; returns exit code, milliseconds and combined output.
function Invoke-Timed([string] $Exe, [string[]] $Arguments) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $Exe @Arguments 2>&1 | Out-String
    $sw.Stop()
    $code = $LASTEXITCODE
    Write-Host "> $Exe $($Arguments -join ' ')  (exit $code, $($sw.ElapsedMilliseconds) ms)"
    Write-Host (($out -split "`r?`n" | Select-Object -Last 40) -join "`n")
    [pscustomobject]@{ Code = $code; Ms = $sw.ElapsedMilliseconds; Out = $out }
}

function Get-Tail([string] $Text, [int] $Lines = 4) {
    ($Text -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last $Lines) -join ' / '
}

# One-line verdict for a DISM run.
function Get-Verdict($Run) {
    $state = if ($Run.Code -in 0, 3010) { 'ok' } else { "exit $($Run.Code)" }
    "$state in $([math]::Round($Run.Ms / 1000, 1)) s"
}

function Get-FeatureState([string] $Text, [string] $Name) {
    $line = $Text -split "`r?`n" | Where-Object { $_ -match "^\s*$Name\s*\|" } | Select-Object -First 1
    if ($line) { ($line -split '\|')[1].Trim() } else { 'absent' }
}

function Write-Report {
    $text = $notes -join "`n"
    if ($env:GITHUB_STEP_SUMMARY) {
        "### S-02 $Experiment`n``````n$text`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append
    }
    $msg = $text.Replace('%', '%25').Replace("`r", '%0D').Replace("`n", '%0A')
    Write-Host "::notice title=S-02 $Experiment::$msg"
}

try {
    switch ($Experiment) {
        'E1' {
            $os = Get-CimInstance Win32_OperatingSystem
            Note "OS: $($os.Caption) $($os.Version), admin=$(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrators'))"
            Get-PSDrive -PSProvider FileSystem | Where-Object Free |
                ForEach-Object { Note "drive $($_.Root) free $([math]::Round($_.Free / 1GB)) GiB" }
            Note "work folder: $work"
            Note "docker: $(docker version --format '{{.Server.Os}} {{.Server.Version}}' 2>&1)"
            Note "docker info: $(docker info --format 'isolation={{.Isolation}} os={{.OSType}} root={{.DockerRootDir}}' 2>&1)"
            $images = docker images --format '{{.Repository}}:{{.Tag}} {{.Size}}' 2>&1
            Note "cached images: $(@($images).Count): $(@($images) -join '; ')"
        }
        'E2' {
            $pull = Invoke-Timed docker 'pull', $base
            Note "pull ${base}: $(Get-Verdict $pull)"
            $null = Invoke-Timed docker 'rm', '-f', 's02'
            $create = Invoke-Timed docker 'create', '--name', 's02', $base, 'cmd'
            if ($create.Code) { throw "docker create: $(Get-Tail $create.Out)" }
            $tar = Join-Path $work 's02.tar'
            $export = Invoke-Timed docker 'export', 's02', '-o', $tar
            Note "export: $(Get-Verdict $export), $([math]::Round((Get-Item $tar).Length / 1GB, 2)) GiB"
            $list = Invoke-Timed $tarExe '-tf', $tar
            $entries = $list.Out -split "`r?`n" | Where-Object { $_ }
            $tops = $entries | ForEach-Object { ($_ -split '/')[0] } | Group-Object | ForEach-Object { "$($_.Name)($($_.Count))" }
            Note "tar: $($entries.Count) entries; top level: $($tops -join ', ')"
            Note "hive-like entries: $(($entries | Where-Object { $_ -match '(?i)(^Hives/|System32/config/(SYSTEM|SOFTWARE|COMPONENTS|DRIVERS)$)' } | Select-Object -First 12) -join ', ')"
            $x = Join-Path $work 'x'
            $null = New-Item -ItemType Directory -Force $x
            $extract = Invoke-Timed $tarExe '-xf', $tar, '-C', $x
            Note "extract: $(Get-Verdict $extract) $(Get-Tail $extract.Out 3)"
            Remove-Item $tar -Force
            $root = if (Test-Path (Join-Path $x 'Files\Windows')) { Join-Path $x 'Files' } else { $x }
            $config = Join-Path $root 'Windows\System32\config'
            $sizes = Get-ChildItem $config -File -ErrorAction SilentlyContinue |
                Where-Object Name -In 'SYSTEM', 'SOFTWARE', 'COMPONENTS', 'DRIVERS', 'SAM', 'SECURITY', 'DEFAULT' |
                ForEach-Object { "$($_.Name)=$([math]::Round($_.Length / 1MB, 1))M" }
            Note "image root: $root; config hives: $($sizes -join ' ')"
            $hives = Join-Path $x 'Hives'
            if (Test-Path $hives) {
                Note "Hives/: $((Get-ChildItem $hives -File | ForEach-Object { "$($_.Name)=$([math]::Round($_.Length / 1MB, 1))M" }) -join ' ')"
                # Layer format: the registry lives in Hives/<Name>_Base; DISM expects it under config.
                $map = @{ System = 'SYSTEM'; Software = 'SOFTWARE'; Sam = 'SAM'; Security = 'SECURITY'; DefaultUser = 'DEFAULT' }
                foreach ($k in $map.Keys) {
                    $src = Join-Path $hives "$($k)_Base"
                    $dst = Join-Path $config $map[$k]
                    if ((Test-Path $src) -and (-not (Test-Path $dst) -or (Get-Item $dst).Length -eq 0)) {
                        Copy-Item $src $dst -Force
                        Note "copied Hives/$($k)_Base -> config/$($map[$k])"
                    }
                }
            }
            Move-Item $root $img
            Note "offline image ready at $img"
        }
        'E3' {
            if (-not (Test-Path $img)) { throw 'no offline image (E2 failed)' }
            $pk = Invoke-Timed dism '/English', "/Image:$img", '/Get-Packages', '/Format:Table'
            Note "Get-Packages: $(Get-Verdict $pk), $(@($pk.Out -split "`n" | Where-Object { $_ -match '~' }).Count) packages $(if ($pk.Code) { Get-Tail $pk.Out })"
            $ft = Invoke-Timed dism '/English', "/Image:$img", '/Get-Features', '/Format:Table'
            Note "Get-Features: $(Get-Verdict $ft), TelnetClient=$(Get-FeatureState $ft.Out 'TelnetClient') $(if ($ft.Code) { Get-Tail $ft.Out })"
            $en = Invoke-Timed dism '/English', "/Image:$img", '/Enable-Feature', '/FeatureName:TelnetClient'
            $after = Invoke-Timed dism '/English', "/Image:$img", '/Get-Features', '/Format:Table'
            Note "Enable TelnetClient: $(Get-Verdict $en) -> $(Get-FeatureState $after.Out 'TelnetClient') $(if ($en.Code -notin 0, 3010) { Get-Tail $en.Out })"
            $dis = Invoke-Timed dism '/English', "/Image:$img", '/Disable-Feature', '/FeatureName:TelnetClient'
            $after = Invoke-Timed dism '/English', "/Image:$img", '/Get-Features', '/Format:Table'
            Note "Disable TelnetClient: $(Get-Verdict $dis) -> $(Get-FeatureState $after.Out 'TelnetClient') $(if ($dis.Code -notin 0, 3010) { Get-Tail $dis.Out })"
            $ch = Invoke-Timed dism '/English', "/Image:$img", '/Cleanup-Image', '/CheckHealth'
            Note "CheckHealth: $(Get-Verdict $ch) $(Get-Tail $ch.Out 2)"
            $load = Invoke-Timed reg 'load', 'HKLM\S02SOFT', (Join-Path $img 'Windows\System32\config\SOFTWARE')
            if (-not $load.Code) {
                $cv = Get-ItemProperty 'HKLM:\S02SOFT\Microsoft\Windows NT\CurrentVersion'
                Note "offline SOFTWARE hive: build $($cv.CurrentBuild).$($cv.UBR), $($cv.ProductName), $($cv.InstallationType)"
                [gc]::Collect()
                $null = Invoke-Timed reg 'unload', 'HKLM\S02SOFT'
            } else { Note "reg load SOFTWARE: exit $($load.Code) $(Get-Tail $load.Out 2)" }
        }
        'E4' {
            if (-not (Test-Path $img)) { throw 'no offline image (E2 failed)' }
            $vol = "${img}:C:\img"
            $ft = Invoke-Timed docker 'run', '--rm', '-v', $vol, $base, 'dism', '/English', '/Image:C:\img', '/Get-Features', '/Format:Table'
            Note "container /Image Get-Features: $(Get-Verdict $ft), TelnetClient=$(Get-FeatureState $ft.Out 'TelnetClient') $(if ($ft.Code) { Get-Tail $ft.Out })"
            $en = Invoke-Timed docker 'run', '--rm', '-v', $vol, $base, 'dism', '/English', '/Image:C:\img', '/Enable-Feature', '/FeatureName:TelnetClient'
            Note "container /Image Enable TelnetClient: $(Get-Verdict $en) $(if ($en.Code -notin 0, 3010) { Get-Tail $en.Out })"
            $dis = Invoke-Timed docker 'run', '--rm', '-v', $vol, $base, 'dism', '/English', '/Image:C:\img', '/Disable-Feature', '/FeatureName:TelnetClient'
            Note "container /Image Disable TelnetClient: $(Get-Verdict $dis) $(if ($dis.Code -notin 0, 3010) { Get-Tail $dis.Out })"
            $on = Invoke-Timed docker 'run', '--rm', $base, 'dism', '/English', '/Online', '/Get-Features', '/Format:Table'
            Note "container /Online Get-Features: $(Get-Verdict $on) $(if ($on.Code) { Get-Tail $on.Out })"
            $onEn = Invoke-Timed docker 'run', '--rm', $base, 'dism', '/English', '/Online', '/Enable-Feature', '/FeatureName:TelnetClient'
            Note "container /Online Enable TelnetClient: $(Get-Verdict $onEn) $(if ($onEn.Code -notin 0, 3010) { Get-Tail $onEn.Out })"
        }
        'E5' {
            $src = Join-Path $work 'small'
            $null = New-Item -ItemType Directory -Force $src
            1..20 | ForEach-Object { Set-Content (Join-Path $src "f$_.txt") ("x" * 4096 * $_) }
            $wim = Join-Path $work 't.wim'
            Remove-Item $wim -ErrorAction SilentlyContinue
            $cap = Invoke-Timed dism '/English', '/Capture-Image', "/ImageFile:$wim", "/CaptureDir:$src", '/Name:t'
            Note "capture: $(Get-Verdict $cap) $(if ($cap.Code) { Get-Tail $cap.Out })"
            $mnt = Join-Path $work 'mnt'
            $null = New-Item -ItemType Directory -Force $mnt
            $mount = Invoke-Timed dism '/English', '/Mount-Image', "/ImageFile:$wim", '/Index:1', "/MountDir:$mnt"
            $seen = @(Get-ChildItem $mnt -ErrorAction SilentlyContinue).Count
            $um = Invoke-Timed dism '/English', '/Unmount-Image', "/MountDir:$mnt", '/Discard'
            Note "runner mount: $(Get-Verdict $mount), $seen files visible, unmount $(Get-Verdict $um) $(if ($mount.Code) { Get-Tail $mount.Out })"
            $inner = 'mkdir C:\m && dism /English /Mount-Image /ImageFile:C:\w\t.wim /Index:1 /MountDir:C:\m && dir /b C:\m && dism /English /Unmount-Image /MountDir:C:\m /Discard'
            $c = Invoke-Timed docker 'run', '--rm', '-v', "${work}:C:\w", $base, 'cmd', '/c', $inner
            Note "container mount: $(Get-Verdict $c) $(Get-Tail $c.Out 3)"
            $ap = Join-Path $work 'apply'
            $null = New-Item -ItemType Directory -Force $ap
            $apply = Invoke-Timed docker 'run', '--rm', '-v', "${work}:C:\w", $base, 'dism', '/English', '/Apply-Image', '/ImageFile:C:\w\t.wim', '/Index:1', '/ApplyDir:C:\w\apply'
            Note "container apply: $(Get-Verdict $apply), $(@(Get-ChildItem $ap).Count) files $(if ($apply.Code) { Get-Tail $apply.Out })"
        }
    }
} catch {
    Note "ERROR: $($_.Exception.Message)"
}
Write-Report
exit 0
