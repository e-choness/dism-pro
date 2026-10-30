<#
Smoke test of the shipped dismpro-cli.exe against the running Windows of a GitHub windows-2025 runner
(online target). The runner VM is discarded after the job, so this makes a real feature change and undoes
it. It doesn't replace the T3 container tests (spike S-02, roadmap 1.5); it's the first time the DISM
calls run on real Windows.

Each check is reported in the log, in the job summary, and as one annotation (readable without signing
in). Exits 1 if any check fails.
#>
param(
    [Parameter(Mandatory)] [string] $Cli,
    # A small optional feature whose payload ships with Windows Server; no restart needed.
    [string] $Feature = 'TelnetClient'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Cli = (Resolve-Path $Cli).Path
$errFile = Join-Path ([IO.Path]::GetTempPath()) 'dismpro-cli.stderr.txt'
$results = [Collections.Generic.List[string]]::new()
$failures = 0

# Runs the CLI with --json and returns the parsed JSON Lines. Throws unless it exits with $Expect.
function Invoke-Cli([string[]] $Arguments, [int] $Expect = 0) {
    $lines = & $Cli @Arguments --json 2> $errFile
    $code = $LASTEXITCODE
    if ($code -ne $Expect) {
        $err = Get-Content $errFile -Raw -ErrorAction SilentlyContinue
        throw "dismpro-cli $($Arguments -join ' ') exited $code, expected $Expect. stdout: $($lines -join ' ') stderr: $err"
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
Test-Step 'purge' {
    $docs = Invoke-Cli 'purge', '--yes', '--allow-irreversible'
    $left = @((Get-Doc (Invoke-Cli 'journal', 'list') 'journals.v1').jobs).Count
    if ($left) { throw "$left journal(s) left after purge" }
    "removed $(@((Get-Doc $docs 'plan.v1').changes).Count) item(s)"
}

$report = $results -join "`n"
$arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($env:GITHUB_STEP_SUMMARY) {
    "### dismpro-cli smoke ($arch runner VM, online)`n``````n$report`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append
}
$kind = if ($failures) { 'error' } else { 'notice' }
Write-Host "::$kind title=dismpro-cli smoke $arch ($failures failed)::$(Format-Annotation $report)"
exit [int] [bool] $failures
