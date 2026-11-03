<#
Smoke test of the shipped dismpro-cli.exe on a GitHub Windows runner VM, which is discarded after the
job, so this makes a real feature change and undoes it. Without -Target it services the running Windows
(online). With -Target it services an offline Windows directory: T3 (roadmap 1.5), whose image comes
from .github/scripts/offline-image.ps1. Spike S-02 found DISM can't service images inside a container,
so T3 runs on the VM.

Each check is reported in the log, in the job summary, and as one annotation (readable without signing
in). Exits 1 if any check fails.
#>
param(
    [Parameter(Mandatory)] [string] $Cli,
    # Offline Windows root passed as --target; empty for the online OS.
    [string] $Target,
    # A small optional feature whose payload ships with Windows Server; no restart needed.
    [string] $Feature = 'TelnetClient'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Cli = (Resolve-Path $Cli).Path
$errFile = Join-Path ([IO.Path]::GetTempPath()) 'dismpro-cli.stderr.txt'
$results = [Collections.Generic.List[string]]::new()
$failures = 0
$targetArgs = if ($Target) { '--target', (Resolve-Path $Target).Path } else { @() }
$targetName = if ($Target) { 'offline image' } else { 'online' }

# Runs the CLI with --json and returns the parsed JSON Lines. Throws unless it exits with one of
# $Expect; $script:lastExit keeps the code.
function Invoke-Cli([string[]] $Arguments, [int[]] $Expect = 0) {
    $lines = & $Cli @Arguments @targetArgs --json 2> $errFile
    $code = $LASTEXITCODE
    $script:lastExit = $code
    if ($code -notin $Expect) {
        $err = Get-Content $errFile -Raw -ErrorAction SilentlyContinue
        throw "dismpro-cli $($Arguments -join ' ') exited $code, expected $($Expect -join ' or '). stdout: $($lines -join ' ') stderr: $err"
    }
    @($lines | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
}

function Get-Doc($Docs, [string] $Schema) {
    $doc = $Docs | Where-Object schema -EQ $Schema | Select-Object -First 1
    if (-not $doc) { throw "no $Schema document in the output" }
    $doc
}

function Get-Feature {
    $f = (Get-Doc (Invoke-Cli 'features', 'list') 'features.v1').features | Where-Object name -EQ $Feature
    if (-not $f) { throw "$Feature isn't in 'features list'" }
    $f
}

# Applies a plan with --yes and checks every change applied.
function Invoke-Job([string[]] $Arguments) {
    $docs = Invoke-Cli ($Arguments + '--yes')
    $plan = Get-Doc $docs 'plan.v1'
    $job = Get-Doc $docs 'job.v1'
    if ($job.status -ne 'completed' -or @($job.failed).Count -or @($job.applied).Count -ne @($plan.changes).Count) {
        throw "job $($job.job): $($job.status), applied $(@($job.applied).Count) of $(@($plan.changes).Count), failed $($job.failed | ConvertTo-Json -Compress)"
    }
    $job
}

# The feature's state after a job; a pending restart leaves it "pending", so that's only reported.
function Assert-Feature([bool] $Enabled, $Job) {
    $f = Get-Feature
    if ($Job.reboot_required) { return "job $($Job.job), restart pending, $Feature is $($f.state)" }
    if ($f.enabled -ne $Enabled) { throw "after job $($Job.job) $Feature is $($f.state), expected enabled=$Enabled" }
    "job $($Job.job), $Feature is $($f.state)"
}

function Test-Step([string] $Name, [scriptblock] $Body) {
    try {
        $line = "PASS ${Name}: $(& $Body)"
    } catch {
        $script:failures++
        $line = "FAIL ${Name}: $($_.Exception.Message)"
    }
    $results.Add($line)
    Write-Host $line
}

function Format-Annotation([string] $Text) { $Text.Replace('%', '%25').Replace("`r", '%0D').Replace("`n", '%0A') }

Test-Step 'targets list' {
    $d = Get-Doc (Invoke-Cli 'targets', 'list') 'targets.v1'
    if (-not ($d.targets | Where-Object kind -EQ 'online')) { throw 'no online target' }
    "$(@($d.targets).Count) target(s): $((@($d.targets).kind | Sort-Object -Unique) -join ', ')"
}
Test-Step 'reboot-reasons' {
    $d = Get-Doc (Invoke-Cli 'reboot-reasons') 'reboot-reasons.v1'
    "pending=$($d.pending) $(@($d.reasons).reason -join ', ')"
}
Test-Step 'packages list' {
    $d = Get-Doc (Invoke-Cli 'packages', 'list') 'packages.v1'
    if (-not @($d.packages).Count) { throw 'no packages' }
    "$(@($d.packages).Count) packages"
}
Test-Step 'features list' {
    $d = Get-Doc (Invoke-Cli 'features', 'list') 'features.v1'
    $script:initial = [bool] (Get-Feature).enabled
    "$(@($d.features).Count) features, $Feature enabled=$initial"
}
Test-Step 'health check' {
    $d = Get-Doc (Invoke-Cli 'health', 'check') 'health.v1'
    "$($d.state), $(@($d.findings).Count) finding(s)"
}

$verb = if ($initial) { 'disable' } else { 'enable' }
Test-Step 'refusals' {
    $null = Get-Doc (Invoke-Cli 'features', $verb, $Feature, '--dry-run', '--yes' -Expect 2) 'error.v1'
    $null = Get-Doc (Invoke-Cli 'features', $verb, $Feature -Expect 3) 'plan.v1'
    $null = Get-Doc (Invoke-Cli 'features', 'enable', 'NoSuchFeature-DismPro', '--yes' -Expect 1) 'error.v1'
    $plan = Get-Doc (Invoke-Cli 'features', $verb, $Feature, '--dry-run') 'plan.v1'
    if (@($plan.changes).Count -ne 1) { throw "dry run planned $(@($plan.changes).Count) changes" }
    if ([bool] (Get-Feature).enabled -ne $initial) { throw 'a refused or dry-run command changed the feature' }
    'usage (2), confirmation (3), unknown name (1), dry run (0, unchanged)'
}
Test-Step "features $verb" {
    $script:jobA = Invoke-Job 'features', $verb, $Feature
    Assert-Feature (-not $initial) $jobA
}
Test-Step 'journal undo' {
    $script:jobB = Invoke-Job 'journal', 'undo', $jobA.job
    if ($jobB.undoes -ne $jobA.job) { throw "undoes=$($jobB.undoes)" }
    Assert-Feature $initial $jobB
}
Test-Step 'journal undo again (refused)' {
    $e = Get-Doc (Invoke-Cli 'journal', 'undo', $jobA.job, '--yes' -Expect 1) 'error.v1'
    $e.message
}
Test-Step 'undo the undo (redo)' {
    $script:jobC = Invoke-Job 'journal', 'undo', $jobB.job
    Assert-Feature (-not $initial) $jobC
}
Test-Step 'journal list' {
    $jobs = (Get-Doc (Invoke-Cli 'journal', 'list') 'journals.v1').jobs
    $a = $jobs | Where-Object job -EQ $jobA.job
    $b = $jobs | Where-Object job -EQ $jobB.job
    if ($a.undone_by -ne $jobB.job -or $b.undone_by -ne $jobC.job) { throw "undo links: $($jobs | ConvertTo-Json -Compress)" }
    "$(@($jobs).Count) jobs, undo chain linked"
}
Test-Step 'journal show' {
    $j = Get-Doc (Invoke-Cli 'journal', 'show', $jobC.job) 'job.v1'
    if ($j.undoes -ne $jobB.job -or $j.status -ne 'completed') { throw ($j | ConvertTo-Json -Compress) }
    "$($j.op), $($j.status)"
}
Test-Step 'restore original state' {
    $job = Invoke-Job 'journal', 'undo', $jobC.job
    Assert-Feature $initial $job
}
# Drivers (TD-0002 §5.6, roadmap 2.5). An unsigned null-driver package: one root-enumerated model
# that installs nothing and copies no file, so staging it changes only the driver store.
$infName = 'dpro-smoke-null.inf'
$infDir = Join-Path ([IO.Path]::GetTempPath()) "dpro-smoke-drivers-$PID"
$null = New-Item -ItemType Directory -Force (Join-Path $infDir 'empty')
@'
; DISM Pro smoke test package: a device that never exists. Never installed.
[Version]
Signature   = "$WINDOWS NT$"
Class       = System
ClassGuid   = {4d36e97d-e325-11ce-bfc1-08002be10318}
Provider    = %Provider%
DriverVer   = 11/03/2026,1.0.0.0
PnpLockdown = 1

[Manufacturer]
%Provider% = Models, NTamd64, NTarm64

[Models.NTamd64]
%Device% = NullInstall, ROOT\DPRO_SMOKE_NULL

[Models.NTarm64]
%Device% = NullInstall, ROOT\DPRO_SMOKE_NULL

[NullInstall.NT]

[NullInstall.NT.Services]
AddService = , 0x00000002

[Strings]
Provider = "DISM Pro smoke"
Device   = "DISM Pro smoke null device"
'@ | Set-Content -Encoding ascii (Join-Path $infDir $infName)

# Our package in the target's store, or $null.
function Get-OurDriver {
    $d = Get-Doc (Invoke-Cli 'drivers', 'list') 'drivers.v1'
    @($d.drivers) | Where-Object { (Split-Path $_.inf -Leaf) -eq $infName } | Select-Object -First 1
}

function Assert-OurDriver([bool] $Staged, $Job) {
    $p = Get-OurDriver
    if ([bool] $p -ne $Staged) { throw "after job $($Job.job) the package is $(if ($p) { "staged as $($p.published)" } else { 'not staged' }), expected staged=$Staged" }
    if ($p) { "job $($Job.job), staged as $($p.published) ($($p.signature))" } else { "job $($Job.job), not staged" }
}

function Get-Median([double[]] $Values) {
    $s = @($Values | Sort-Object)
    $mid = [int][Math]::Floor($s.Count / 2)
    if ($s.Count % 2) { $s[$mid] } else { ($s[$mid - 1] + $s[$mid]) / 2 }
}

Test-Step 'drivers list' {
    $d = Get-Doc (Invoke-Cli 'drivers', 'list') 'drivers.v1'
    if (Get-OurDriver) { throw "$infName is already staged: a previous run didn't clean up" }
    "$(@($d.drivers).Count) third-party package(s)"
}

# G1 ratio gate (TD-0002 §7): the whole command against dism.exe on the same VM and target. One
# warm-up each, then 5 interleaved runs; the medians' ratio must be at most 1.3. Offline lists the
# inbox packages too (2.5 s, 246 packages); online stays third-party (inbox online takes 17-30 s).
Test-Step 'drivers list vs dism.exe /Get-Drivers (gate 1.3x)' {
    # @(...) keeps it an array: an `if` unwraps a one-item array to a string, and splatting a
    # string passes each character as an argument.
    $all = @(if ($Target) { '--all' })
    $dismArgs = if ($Target) { @("/Image:$((Resolve-Path $Target).Path)", '/Get-Drivers', '/All') } else { @('/Online', '/Get-Drivers') }
    $dism = Join-Path $env:SystemRoot 'System32\dism.exe'
    $ours = {
        $null = & $Cli 'drivers' 'list' @all @targetArgs '--json' 2> $null
        if ($LASTEXITCODE) { throw "dismpro-cli drivers list exited $LASTEXITCODE" }
    }
    $theirs = {
        $null = & $dism @dismArgs '/English'
        if ($LASTEXITCODE) { throw "dism.exe $($dismArgs -join ' ') exited $LASTEXITCODE" }
    }
    & $ours
    & $theirs
    $a = [Collections.Generic.List[double]]::new()
    $b = [Collections.Generic.List[double]]::new()
    for ($i = 0; $i -lt 5; $i++) {
        $a.Add((Measure-Command $ours).TotalMilliseconds)
        $b.Add((Measure-Command $theirs).TotalMilliseconds)
    }
    $ma = Get-Median $a
    $mb = Get-Median $b
    $ratio = $ma / $mb
    $text = 'ratio {0:N2} (median {1:N0} ms vs {2:N0} ms{3})' -f $ratio, $ma, $mb, $(if ($Target) { ', --all' } else { '' })
    if ($ratio -gt 1.3) { throw "over the gate: $text" }
    $text
}

if (-not $Target) {
    Test-Step 'drivers add online (refused)' {
        $e = Get-Doc (Invoke-Cli 'drivers', 'add', $infDir, '--force-unsigned', '--yes' -Expect 1) 'error.v1'
        if (Get-OurDriver) { throw 'a refused add staged the package' }
        $e.message
    }
} else {
    Test-Step 'drivers add refusals' {
        $e = Get-Doc (Invoke-Cli 'drivers', 'add', $infDir, '--yes' -Expect 1) 'error.v1'
        if ($e.message -notmatch 'force-unsigned') { throw "unsigned refusal: $($e.message)" }
        $null = Get-Doc (Invoke-Cli 'drivers', 'add', (Join-Path $infDir 'empty'), '--force-unsigned', '--yes' -Expect 1) 'error.v1'
        $null = Get-Doc (Invoke-Cli 'drivers', 'remove', 'oem99999.inf', '--yes' -Expect 1) 'error.v1'
        $plan = Get-Doc (Invoke-Cli 'drivers', 'add', $infDir, '--force-unsigned', '--dry-run') 'plan.v1'
        if (@($plan.changes).Count -ne 1) { throw "dry run planned $(@($plan.changes).Count) changes" }
        if (Get-OurDriver) { throw 'a refused or dry-run command staged the package' }
        'unsigned (1), folder without INFs (1), unknown package (1), dry run (0, unchanged)'
    }
    Test-Step 'drivers add' {
        $script:drvA = Invoke-Job 'drivers', 'add', $infDir, '--force-unsigned'
        Assert-OurDriver $true $drvA
    }
    Test-Step 'drivers add again (already staged)' {
        $plan = Get-Doc (Invoke-Cli 'drivers', 'add', (Join-Path $infDir $infName), '--force-unsigned', '--dry-run') 'plan.v1'
        if (@($plan.changes).Count) { throw "planned $(@($plan.changes).Count) change(s) for a package the store has" }
        'nothing to do'
    }
    # A step whose input job is missing says so, rather than failing as a usage error.
    function Assert-Prior($Job, [string] $Step) {
        if (-not $Job) { throw "skipped: '$Step' produced no job" }
    }
    Test-Step 'undo the add (learned inverse)' {
        Assert-Prior $drvA 'drivers add'
        $script:drvB = Invoke-Job 'journal', 'undo', $drvA.job
        Assert-OurDriver $false $drvB
    }
    Test-Step 'undo the undo (re-add from quarantine)' {
        Assert-Prior $drvB 'undo the add'
        $script:drvC = Invoke-Job 'journal', 'undo', $drvB.job
        $root = Get-Item -Force (Join-Path (Resolve-Path $Target).Path '$DismProQuarantine')
        if (-not ($root.Attributes -band [IO.FileAttributes]::Hidden)) { throw "$($root.FullName) isn't hidden" }
        $copy = Join-Path $root.FullName "$($drvB.job)\drivers"
        if (-not (Get-ChildItem $copy -Recurse -Filter $infName -ErrorAction SilentlyContinue)) { throw "no store copy under $copy" }
        Assert-OurDriver $true $drvC
    }
    # DISM counts a root-enumerated System-class package as boot-critical (run 41720190520), so
    # the removal is refused first and then confirmed with --allow-boot-critical.
    Test-Step 'drivers remove' {
        $ours = Get-OurDriver
        if (-not $ours) { throw "skipped: $infName isn't staged" }
        $flags = @()
        $said = "boot_critical=$($ours.boot_critical)"
        if ($ours.boot_critical) {
            $e = Get-Doc (Invoke-Cli 'drivers', 'remove', $ours.published, '--yes' -Expect 1) 'error.v1'
            if ($e.message -notmatch 'allow-boot-critical') { throw "boot-critical refusal: $($e.message)" }
            if (-not (Get-OurDriver)) { throw 'a refused removal removed the package' }
            $flags = @('--allow-boot-critical')
            $said += ', refused without --allow-boot-critical (1)'
        }
        $script:drvD = Invoke-Job (@('drivers', 'remove', $ours.published) + $flags)
        "${said}; $(Assert-OurDriver $false $drvD)"
    }
    Test-Step 'undo the removal' {
        Assert-Prior $drvD 'drivers remove'
        $script:drvE = Invoke-Job 'journal', 'undo', $drvD.job
        Assert-OurDriver $true $drvE
    }
    Test-Step 'undo again (store as it was)' {
        Assert-Prior $drvE 'undo the removal'
        $job = Invoke-Job 'journal', 'undo', $drvE.job
        Assert-OurDriver $false $job
    }

    # Packages (TD-0002, roadmap 2.5): a removal is permanent, so it needs --allow-irreversible and
    # has no undo. The image goes away with the VM, so the step removes the first package DISM
    # accepts, Features on Demand first; CBS refuses permanent packages (DismReleaseType: 14
    # OnDemandPack, 8 LanguagePack, 2 FeaturePack; 9 Foundation is never tried).
    function Get-Package([string] $Identity) {
        @((Get-Doc (Invoke-Cli 'packages', 'list') 'packages.v1').packages) | Where-Object identity -EQ $Identity
    }
    $script:pkgJob = $null
    Test-Step 'packages remove (irreversible)' {
        $installed = @((Get-Doc (Invoke-Cli 'packages', 'list') 'packages.v1').packages | Where-Object state -EQ 'installed')
        $kinds = ($installed | Group-Object release_type | ForEach-Object { "$($_.Name) x$($_.Count)" }) -join ', '
        $candidates = @(foreach ($t in 14, 8, 2) { $installed | Where-Object release_type -EQ $t }) | Select-Object -First 4
        if (-not $candidates) { throw "no candidate among $($installed.Count) installed packages (release types: $kinds)" }
        $plan = Get-Doc (Invoke-Cli 'packages', 'remove', $candidates[0].identity, '--yes' -Expect 3) 'plan.v1'
        if ($plan.summary.irreversible -ne 1) { throw "without --allow-irreversible: $($plan.summary | ConvertTo-Json -Compress)" }
        $tried = [Collections.Generic.List[string]]::new()
        foreach ($p in $candidates) {
            $docs = Invoke-Cli 'packages', 'remove', $p.identity, '--yes', '--allow-irreversible' -Expect 0, 1
            $job = Get-Doc $docs 'job.v1'
            if ($script:lastExit -eq 0 -and @($job.applied).Count -eq 1) {
                $script:pkgJob = $job
                $script:pkgId = $p.identity
                $script:pkgType = $p.release_type
                break
            }
            $tried.Add("$($p.identity) (type $($p.release_type)): exit $script:lastExit, failed [$(@($job.failed | ForEach-Object code) -join ', ')]")
        }
        if (-not $script:pkgJob) { throw "DISM removed none of them: $($tried -join '; ') (release types: $kinds)" }
        $after = Get-Package $pkgId
        $state = if ($after) { $after.state } else { 'gone' }
        if ($state -eq 'installed') { throw "job $($pkgJob.job) applied, yet $pkgId is still installed" }
        $refused = if ($tried.Count) { "; refused first: $($tried -join '; ')" } else { '' }
        "exit 3 without --allow-irreversible; removed $pkgId (type $pkgType) in job $($pkgJob.job), now $state, reboot_required=$($pkgJob.reboot_required)$refused; release types: $kinds"
    }
    if ($script:pkgJob) {
        Test-Step 'packages remove has no undo' {
            $docs = Invoke-Cli 'journal', 'undo', $pkgJob.job, '--yes' -Expect 0, 1
            $plan = $docs | Where-Object schema -EQ 'plan.v1' | Select-Object -First 1
            $undo = $docs | Where-Object schema -EQ 'job.v1' | Select-Object -First 1
            if ($undo -and @($undo.applied).Count) { throw "the undo applied $(@($undo.applied).Count) change(s)" }
            $after = Get-Package $pkgId
            if ($after -and $after.state -eq 'installed') { throw "$pkgId is installed again" }
            $said = if ($plan) { "$(@($plan.changes).Count) change(s) planned" } else { (Get-Doc $docs 'error.v1').message }
            "exit $script:lastExit, $said; $pkgId stays $(if ($after) { $after.state } else { 'gone' })"
        }
    }
}
Remove-Item -Recurse -Force $infDir -ErrorAction SilentlyContinue

Test-Step 'purge' {
    $docs = Invoke-Cli 'purge', '--yes', '--allow-irreversible'
    $left = @((Get-Doc (Invoke-Cli 'journal', 'list') 'journals.v1').jobs).Count
    if ($left) { throw "$left journal(s) left after purge" }
    "removed $(@((Get-Doc $docs 'plan.v1').changes).Count) item(s)"
}

$report = $results -join "`n"
$arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($env:GITHUB_STEP_SUMMARY) {
    "### dismpro-cli smoke ($arch runner VM, $targetName)`n``````n$report`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append
}
$kind = if ($failures) { 'error' } else { 'notice' }
Write-Host "::$kind title=dismpro-cli smoke $arch $targetName ($failures failed)::$(Format-Annotation $report)"
exit [int] [bool] $failures
