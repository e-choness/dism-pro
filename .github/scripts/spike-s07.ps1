<#
Spike S-07 (roadmap 1.10, TD-0015 §5.7) on the windows-2025 runner VM: can an install be tracked by
ETW kernel providers (file, registry, process), the USN delta and a registry last-write diff, and are
writes by the Windows Installer service attributed? Each step writes its report to the job summary and
one annotation per report (annotations are readable without signing in). Steps never fail the job: a
failure is reported.

  -Step Setup      runner facts; C:\s07-existing\config.ini (the fake installer appends to it);
                   WiX v5 (MS-RL) and the test MSI (1000 files, 50 keys, a Run value)
  -Step Probe      tracker probe: provider keywords, a 2 s session, USN, the MSI mutex
  -Step Fake       bench + check of the fake installer (parent exits, a child writes), with a noise
                   writer running in another process
  -Step Msi        bench + check of the MSI (the Windows Installer service writes), same noise
  -Step Container  tracker probe and a short fake bench inside a servercore container
#>
param([Parameter(Mandatory)] [string] $Step)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$tracker = (Resolve-Path ($env:TRACKER ?? 'dist-s07/tracker.exe')).Path
$work = 'D:\s07'
$msi = Join-Path $work 's07.msi'
$existing = 'C:\s07-existing\config.ini'
$noiseDir = 'C:\s07-noise'
$null = New-Item -ItemType Directory -Force $work

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

# Job summary + one annotation per report.
function Publish([string] $Title, [string] $Text, [switch] $Failed) {
    $Text = $Text.TrimEnd()
    Write-Host $Text
    "## $Title`n`n$Text`n" | Add-Content $env:GITHUB_STEP_SUMMARY
    $kind = $Failed ? 'warning' : 'notice'
    $t = (Format-Annotation "S-07 $Title").Replace(':', '%3A').Replace(',', '%2C')
    Write-Host "::$kind title=$t::$(Format-Annotation $Text)"
}

# Runs the tracker; returns its output with the exit status and wall time.
function Invoke-Tracker([string[]] $Arguments) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $tracker @Arguments 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = "(tracker $($Arguments -join ' '): exit $code, $([math]::Round($sw.Elapsed.TotalSeconds, 1)) s)"
    [pscustomobject]@{ Ok = $code -eq 0; Text = "$($out.TrimEnd())`n`n$tail" }
}

function Publish-Tracker([string] $Title, [string[]] $Arguments) {
    $r = Invoke-Tracker $Arguments
    Publish $Title $r.Text -Failed:(-not $r.Ok)
}

# Another process writing a file every 50 ms under $noiseDir (and a value under HKCU) while an
# install runs: what attribution must leave out.
function Start-Noise {
    $null = New-Item -ItemType Directory -Force $noiseDir
    $script = Join-Path $work 'noise.ps1'
    Set-Content $script @"
`$i = 0
while (`$true) {
    Set-Content -Path "$noiseDir\n`$(`$i % 200).tmp" -Value `$i
    Set-ItemProperty -Path 'HKCU:\Software' -Name 'dpro-noise' -Value `$i
    `$i++
    Start-Sleep -Milliseconds 50
}
"@
    Start-Process pwsh -ArgumentList '-NoProfile', '-File', $script -PassThru -WindowStyle Hidden
}

function Stop-Noise($Process) {
    if ($Process -and -not $Process.HasExited) { Stop-Process -Id $Process.Id -Force }
    Remove-ItemProperty -Path 'HKCU:\Software' -Name 'dpro-noise' -ErrorAction SilentlyContinue
}

switch ($Step) {
    'Setup' {
        $lines = @()
        try {
            $os = Get-CimInstance Win32_OperatingSystem
            $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
            $lines += "OS: $($os.Caption) $($os.Version); CPU: $($cpu.Name), $([Environment]::ProcessorCount) logical; RAM $([math]::Round($os.TotalVisibleMemorySize / 1MB, 1)) GiB"
            $mp = Get-MpComputerStatus -ErrorAction SilentlyContinue
            $lines += "Defender: service $(if ($mp) { 'running' } else { 'unknown' }), real-time protection $($mp.RealTimeProtectionEnabled)"
            $usn = fsutil usn queryjournal C: 2>&1 | Out-String
            $lines += "USN journal C: $(if ($LASTEXITCODE) { "query failed: $($usn.Trim())" } else { (($usn -split "`r?`n") | Where-Object { $_ -match 'Maximum Size|Allocation Delta' }) -join '; ' })"
            $msiserver = Get-Service msiserver
            $lines += "msiserver: $($msiserver.Status), start type $($msiserver.StartType)"

            $null = New-Item -ItemType Directory -Force (Split-Path $existing)
            Set-Content $existing "[s07]`nexisting=1"

            $sw = [Diagnostics.Stopwatch]::StartNew()
            $out = dotnet tool install --global wix --version 5.0.2 2>&1 | Out-String
            if ($LASTEXITCODE) { throw "wix install: $($out.Trim())" }
            $wix = Join-Path $env:USERPROFILE '.dotnet\tools\wix.exe'
            $lines += "WiX: $(& $wix --version) in $([math]::Round($sw.Elapsed.TotalSeconds)) s"

            $src = Join-Path $work 'msi'
            $r = Invoke-Tracker @('make-msi', $src, '--files', '1000', '--keys', '50')
            if (-not $r.Ok) { throw "make-msi: $($r.Text)" }
            $sw.Restart()
            $out = & $wix build -arch x64 (Join-Path $src 's07.wxs') -o $msi 2>&1 | Out-String
            if ($LASTEXITCODE) { throw "wix build: $($out.Trim())" }
            $lines += "MSI: $([math]::Round((Get-Item $msi).Length / 1MB, 2)) MiB, built in $([math]::Round($sw.Elapsed.TotalSeconds)) s"
            Publish 'setup' ($lines -join "`n")
        } catch {
            Publish 'setup' (($lines + "FAILED: $($_.Exception.Message)") -join "`n") -Failed
        }
    }
    'Probe' { Publish-Tracker 'probe' @('probe') }
    'Fake' {
        $noise = Start-Noise
        try {
            $common = @('fake', '--files', '1000', '--keys', '50', '--modify', $existing)
            Publish-Tracker 'fake bench' (@('bench') + $common + @('--runs', '5'))
            Publish-Tracker 'fake check lean' (@('check') + $common + @('--profile', 'lean'))
            Publish-Tracker 'fake check full' (@('check') + $common + @('--profile', 'full'))
        } finally { Stop-Noise $noise }
    }
    'Msi' {
        if (-not (Test-Path $msi)) {
            Publish 'msi' "no MSI at $msi (see the setup step)" -Failed
            break
        }
        $noise = Start-Noise
        try {
            $common = @('msi', '--msi', $msi, '--files', '1000', '--keys', '50')
            Publish-Tracker 'msi bench' (@('bench') + $common + @('--runs', '5'))
            Publish-Tracker 'msi check lean' (@('check') + $common + @('--profile', 'lean'))
            Publish-Tracker 'msi check full' (@('check') + $common + @('--profile', 'full'))
        } finally { Stop-Noise $noise }
    }
    'Container' {
        $image = 'mcr.microsoft.com/windows/servercore:ltsc2025'
        $sw = [Diagnostics.Stopwatch]::StartNew()
        docker pull -q $image | Out-Null
        $pull = "pull $image`: exit $LASTEXITCODE in $([math]::Round($sw.Elapsed.TotalSeconds)) s"
        $dir = Split-Path $tracker
        $name = Split-Path $tracker -Leaf
        $run = @('run', '--rm', '--user', 'ContainerAdministrator', '-v', "${dir}:C:\s07", $image, "C:\s07\$name")
        $probe = docker @run probe 2>&1 | Out-String
        $probeExit = $LASTEXITCODE
        $bench = docker @run bench fake --runs 3 --modes 'none,lean' --files 1000 --keys 50 --root 'C:\S07Fake' 2>&1 | Out-String
        $benchExit = $LASTEXITCODE
        Publish 'container' "$pull`n`n### probe (exit $probeExit)`n$($probe.Trim())`n`n### fake bench (exit $benchExit)`n$($bench.Trim())" -Failed:($probeExit -or $benchExit)
    }
    default { throw "unknown step $Step" }
}
exit 0
