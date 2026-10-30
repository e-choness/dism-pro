<#
Spike S-04 (roadmap 1.7) on the windows-2025 runner VM: NTFS enumeration methods on a synthetic
1.2 M-file corpus (doc 04 §2 "Cleanup scan"), the system volume, stale-plan detection (TD-0001 §12)
and what a container may do. Each step writes its report to the job summary and one annotation
(annotations are readable without signing in). Steps never fail the job: a failure is reported.

  -Step Setup        runner facts; a dedicated NTFS volume (VHDX on D:) as S:, 8.3 names off
  -Step Corpus       fsscan make-corpus S:\c, then a USN journal on S:
  -Step Warm|Cold    fsscan bench on the corpus (cold: volume dismounted before every run)
  -Step System       fsscan bench on C:\Windows\WinSxS, C:\Windows and all of C:
  -Step Fingerprint  USN cursor vs re-stat on the corpus; USN noise on C:
  -Step Container    fsscan probe inside a nanoserver container
#>
param([Parameter(Mandatory)] [string] $Step)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$fsscan = (Resolve-Path ($env:FSSCAN ?? 'dist-s04/fsscan-x64.exe')).Path
$work = 'D:\s04'
$vhdx = Join-Path $work 'corpus.vhdx'
$letter = 'S'
$corpus = "${letter}:\c"
$files = 1200000

function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '').Replace("`n", '%0A')
}

# Job summary + one annotation per report.
function Publish([string] $Title, [string] $Text, [switch] $Failed) {
    $Text = $Text.TrimEnd()
    Write-Host $Text
    "## $Title`n`n$Text`n" | Add-Content $env:GITHUB_STEP_SUMMARY
    $kind = $Failed ? 'warning' : 'notice'
    $t = (Format-Annotation "S-04 $Title").Replace(':', '%3A').Replace(',', '%2C')
    Write-Host "::$kind title=$t::$(Format-Annotation $Text)"
}

# Runs fsscan; returns its output with the exit status and wall time.
function Invoke-Fsscan([string[]] $Arguments) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $fsscan @Arguments 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = "(fsscan $($Arguments[0]): exit $code, $([math]::Round($sw.Elapsed.TotalSeconds, 1)) s)"
    [pscustomobject]@{ Ok = $code -eq 0; Text = "$($out.TrimEnd())`n$tail" }
}

function Publish-Fsscan([string] $Title, [string[]] $Arguments) {
    $r = Invoke-Fsscan $Arguments
    Publish $Title $r.Text -Failed:(-not $r.Ok)
}

switch ($Step) {
    'Setup' {
        $lines = @()
        $os = Get-CimInstance Win32_OperatingSystem
        $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
        $lines += "OS: $($os.Caption) $($os.Version); CPU: $($cpu.Name), $([Environment]::ProcessorCount) logical; RAM $([math]::Round($os.TotalVisibleMemorySize / 1MB, 1)) GiB"
        foreach ($d in Get-Volume | Where-Object DriveLetter) {
            $lines += "volume $($d.DriveLetter): $($d.FileSystem), $([math]::Round($d.Size / 1GB)) GiB, $([math]::Round($d.SizeRemaining / 1GB)) GiB free"
        }
        try {
            $mp = Get-MpComputerStatus
            $lines += "Defender: service $($mp.AMServiceEnabled), real-time $($mp.RealTimeProtectionEnabled)"
        } catch { $lines += "Defender: not available ($($_.Exception.Message))" }
        $lines += "8.3 names: $((fsutil 8dot3name query | Out-String).Trim() -replace '\s+', ' ')"

        $null = New-Item -ItemType Directory -Force $work
        $script = Join-Path $work 'diskpart.txt'
        @(
            "create vdisk file=`"$vhdx`" maximum=65536 type=expandable"
            "select vdisk file=`"$vhdx`""
            'attach vdisk'
            'convert gpt'
            'create partition primary'
            'format fs=ntfs quick label=s04'
            "assign letter=$letter"
        ) | Set-Content $script -Encoding ascii
        $dp = diskpart /s $script | Out-String
        if ($LASTEXITCODE -or -not (Test-Path "${letter}:\")) {
            Publish 'setup' (($lines + "diskpart failed ($LASTEXITCODE):", $dp) -join "`n") -Failed
            exit 0
        }
        fsutil 8dot3name set "${letter}:" 1 | Out-Null
        try { Add-MpPreference -ExclusionPath "${letter}:\"; $lines += "Defender exclusion added for ${letter}:\ (corpus creation speed)" } catch {}
        $info = fsutil fsinfo ntfsinfo "${letter}:" | Select-String 'Bytes Per Cluster|Bytes Per FileRecord|Mft Valid Data Length|NTFS Version'
        $lines += "corpus volume ${letter}: 64 GiB expandable VHDX on D:, NTFS: " + (($info | ForEach-Object { ($_.Line -replace '\s+', ' ').Trim() }) -join '; ')
        Publish 'setup' ($lines -join "`n")
    }
    'Corpus' {
        $r = Invoke-Fsscan @('make-corpus', $corpus, '--files', $files)
        $journal = fsutil usn createjournal m=0x20000000 a=0x4000000 "${letter}:" 2>&1 | Out-String
        $probe = Invoke-Fsscan @('probe', $corpus)
        Publish 'corpus' "$($r.Text)`nUSN journal: $($journal.Trim())`n$($probe.Text)" -Failed:(-not ($r.Ok -and $probe.Ok))
    }
    'Warm' { Publish-Fsscan 'warm' @('bench', $corpus, '--corpus', $files, '--runs', '5') }
    'Cold' { Publish-Fsscan 'cold' @('bench', $corpus, '--corpus', $files, '--runs', '5', '--cold') }
    'System' {
        Publish-Fsscan 'probe C:' @('probe', 'C:\Windows')
        Publish-Fsscan 'WinSxS' @('bench', 'C:\Windows\WinSxS', '--runs', '3')
        Publish-Fsscan 'C:\Windows' @('bench', 'C:\Windows', '--runs', '3')
        Publish-Fsscan 'all of C:' @('bench', 'C:\', '--runs', '3', '--methods', 'find-mt,dirinfo-mt,usn,mft')
    }
    'Fingerprint' {
        Publish-Fsscan 'stale plan' @('fingerprint', $corpus, '--paths', '100000', '--changes', '100')
        Publish-Fsscan 'USN noise C:' @('usn-noise', 'C:\', '--seconds', '30')
    }
    'Container' {
        $image = 'mcr.microsoft.com/windows/nanoserver:ltsc2025'
        $sw = [Diagnostics.Stopwatch]::StartNew()
        docker pull -q $image | Out-Null
        $pull = "pull $image`: exit $LASTEXITCODE in $([math]::Round($sw.Elapsed.TotalSeconds)) s"
        $dir = Split-Path $fsscan
        $name = Split-Path $fsscan -Leaf
        $own = docker run --rm --user ContainerAdministrator -v "${dir}:C:\s04" $image "C:\s04\$name" probe 'C:\Windows' 2>&1 | Out-String
        $bind = docker run --rm --user ContainerAdministrator -v "${dir}:C:\s04" $image "C:\s04\$name" probe 'C:\s04' 2>&1 | Out-String
        Publish 'container' "$pull`n`ncontainer volume:`n$($own.Trim())`n`nbind mount:`n$($bind.Trim())"
    }
    default { throw "unknown step $Step" }
}
exit 0
