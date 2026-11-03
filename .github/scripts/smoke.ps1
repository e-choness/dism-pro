<#
Smoke test of the shipped dismpro-cli.exe on a GitHub Windows runner VM, which is discarded after the
job, so this makes a real feature change and undoes it. Without -Target it services the running Windows
(online). With -Target it services an offline Windows directory: T3 (roadmap 1.5), whose image comes
from .github/scripts/offline-image.ps1. Spike S-02 found DISM can't service images inside a container,
so T3 runs on the VM.

With -Health it runs the Health & Repair checks instead (roadmap 2.6, TD-0006 §10): the full scan and,
on an offline image, the repair. They take minutes, so CI runs them in a job of their own.

Each check is reported in the log, in the job summary, and as one annotation (readable without signing
in). Exits 1 if any check fails.
#>
param(
    [Parameter(Mandatory)] [string] $Cli,
    # Offline Windows root passed as --target; empty for the online OS.
    [string] $Target,
    # A small optional feature whose payload ships with Windows Server; no restart needed.
    [string] $Feature = 'TelnetClient',
    [switch] $Health
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
# $Expect; the code is left in $script:lastExit.
function Invoke-Cli([string[]] $Arguments, [int[]] $Expect = @(0)) {
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

# Reports the results in the job summary and as one annotation, then exits 1 if any check failed.
function Complete-Run([string] $Suite) {
    $report = $results -join "`n"
    $arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($env:GITHUB_STEP_SUMMARY) {
        "### dismpro-cli $Suite ($arch runner VM, $targetName)`n``````n$report`n``````" | Out-File $env:GITHUB_STEP_SUMMARY -Append
    }
    $kind = if ($failures) { 'error' } else { 'notice' }
    Write-Host "::$kind title=dismpro-cli $Suite $arch $targetName ($failures failed)::$(Format-Annotation $report)"
    exit [int] [bool] $failures
}

function Format-Duration([Diagnostics.Stopwatch] $Watch) { '{0:N1} min' -f $Watch.Elapsed.TotalMinutes }

if ($Health) {
    # Which CBS.log a check writes decides where the CLI looks for findings: the target's own (what it
    # reads today) or the runner's, when DISM services an offline image with the runner's stack.
    $windows = if ($Target) { Join-Path (Resolve-Path $Target).Path 'Windows' } else { $env:SystemRoot }
    $logs = [ordered] @{
        'target CBS.log' = Join-Path $windows 'Logs\CBS\CBS.log'
        'runner CBS.log' = Join-Path $env:SystemRoot 'Logs\CBS\CBS.log'
        'runner dism.log' = Join-Path $env:SystemRoot 'Logs\DISM\dism.log'
    }
    if (-not $Target) { $logs.Remove('target CBS.log') }
    function Get-LogSize([string] $Path) { if (Test-Path $Path) { (Get-Item $Path).Length } else { -1 } }

    # An offline check writes neither CBS.log above (runs 41637221034, 41640930548). DISM's scratch
    # folder goes away when its session closes, so a thread polls for logs while the scan runs and
    # keeps a copy of any CBS log it sees; the step reports where they were and their error lines.
    function Start-LogWatch {
        $state = [hashtable]::Synchronized(@{ seen = @{}; copies = @{}; stop = $false })
        $holding = Join-Path ([IO.Path]::GetTempPath()) 'dpro-log-watch'
        $roots = @([IO.Path]::GetTempPath(), (Join-Path $env:SystemRoot 'Logs'), (Join-Path $windows 'Logs'))
        $job = Start-ThreadJob -ArgumentList $state, $roots, $holding -ScriptBlock {
            param($State, $Roots, $Holding)
            $null = New-Item -ItemType Directory -Force $Holding
            $start = (Get-Date).AddSeconds(-2)
            while (-not $State.stop) {
                foreach ($root in $Roots) {
                    Get-ChildItem $root -Recurse -File -Filter *.log -ErrorAction SilentlyContinue |
                        Where-Object { $_.LastWriteTime -ge $start -and $_.FullName -notlike "$Holding*" } |
                        ForEach-Object {
                            $State.seen[$_.FullName] = [math]::Max([long] $State.seen[$_.FullName], $_.Length)
                            if ($_.Name -like 'CBS*.log') {
                                $copy = Join-Path $Holding "$([Math]::Abs($_.FullName.GetHashCode())).log"
                                try {
                                    $src = [IO.File]::Open($_.FullName, 'Open', 'Read', 'ReadWrite, Delete')
                                    try { $dst = [IO.File]::Create($copy); $src.CopyTo($dst); $dst.Dispose() } finally { $src.Dispose() }
                                    $State.copies[$_.FullName] = $copy
                                } catch { }
                            }
                        }
                }
                Start-Sleep -Milliseconds 500
            }
        }
        @{ state = $state; job = $job }
    }

    function Stop-LogWatch($Watch) {
        $Watch.state.stop = $true
        $null = Wait-Job $Watch.job -Timeout 30
        Remove-Job $Watch.job -Force
        $lines = @(foreach ($e in ($Watch.state.seen.GetEnumerator() | Sort-Object Name)) { "$($e.Name): $($e.Value) B" })
        foreach ($e in $Watch.state.copies.GetEnumerator()) {
            $hits = @(Select-String -Path $e.Value -Pattern ', (Error|Warning) |corrupt|Summary|HRESULT' |
                Select-Object -First 25 | ForEach-Object { $_.Line.Trim() })
            $lines += "--- $($e.Name): $($hits.Count) matching line(s)"
            $lines += $hits
        }
        $dism = $logs['runner dism.log']
        if ((Get-LogSize $dism) -gt $script:dismBefore) {
            $s = [IO.File]::Open($dism, 'Open', 'Read', 'ReadWrite, Delete')
            try {
                $null = $s.Seek([math]::Max($script:dismBefore, 0), 'Begin')
                $lines += '--- runner dism.log, written during the scan:'
                $lines += ([IO.StreamReader]::new($s)).ReadToEnd().TrimEnd()
            } finally { $s.Dispose() }
        }
        Write-Host "::notice title=offline health scan logs::$(Format-Annotation ($lines -join "`n"))"
    }

    $script:scanState = $null
    Test-Step 'health scan' {
        $before = @{}; foreach ($k in $logs.Keys) { $before[$k] = Get-LogSize $logs[$k] }
        $script:dismBefore = $before['runner dism.log']
        $watch = if ($Target) { Start-LogWatch }
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try {
            $d = Get-Doc (Invoke-Cli 'health', 'scan') 'health.v1'
        } finally {
            if ($watch) { Stop-LogWatch $watch }
        }
        $took = Format-Duration $sw
        $growth = foreach ($k in $logs.Keys) {
            $after = Get-LogSize $logs[$k]
            if ($after -lt 0) { "$k missing" } else { "$k +$($after - [math]::Max($before[$k], 0)) B" }
        }
        $script:scanState = $d.state
        $kinds = @($d.findings | Group-Object diagnosis | ForEach-Object { "$($_.Name) x$($_.Count)" }) -join ', '
        $summary = "$($d.state), $(@($d.findings).Count) finding(s) [$kinds], $took; $($growth -join ', ')"
        # Any state is the store's business (a container base layer is trimmed, so it can be
        # repairable); a damaged state that no finding explains is ours (TD-0006 §5.2).
        if ($d.state -ne 'healthy' -and -not @($d.findings).Count) { throw "nothing explains it: $summary" }
        $summary
    }
    if ($Target) {
        $script:repaired = $false
        # A restore either repairs the store or, without a usable source, fails with 0x800f0915 ("The
        # repair content could not be found anywhere") as one failed change in a recorded job.
        function Invoke-Restore([string[]] $Extra) {
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $docs = Invoke-Cli (@('health', 'restore', '--limit-access', '--yes', '--allow-irreversible') + $Extra) -Expect 0, 1
            $took = Format-Duration $sw
            $job = Get-Doc $docs 'job.v1'
            $codes = @($job.failed | ForEach-Object code)
            $shape = "job $($job.job) $($job.status), applied $(@($job.applied).Count), failed [$($codes -join ', ')], exit $script:lastExit, $took"
            if ($job.status -ne 'completed') { throw $shape }
            if ($script:lastExit -eq 0 -and @($job.applied).Count -eq 1 -and -not $codes.Count) {
                $script:repaired = $true
                return "repaired, reboot_required=$($job.reboot_required): $shape"
            }
            if ($script:lastExit -eq 1 -and $codes.Count -eq 1 -and $codes[0] -eq '0x800f0915') {
                return "no usable repair source: $shape"
            }
            throw $shape
        }
        Test-Step 'health restore needs --allow-irreversible' {
            $plan = Get-Doc (Invoke-Cli 'health', 'restore', '--limit-access', '--yes' -Expect 3) 'plan.v1'
            if ($plan.summary.irreversible -ne 1) { throw "summary $($plan.summary | ConvertTo-Json -Compress)" }
            "exit 3, $($plan.summary.changes) change(s) planned, $($plan.summary.irreversible) permanent"
        }
        Test-Step 'health restore --limit-access' { Invoke-Restore @() }
        if (-not $script:repaired) {
            # The runner's own Windows as the source: it repairs only if its build matches the image's.
            Test-Step 'health restore --source <runner Windows>' { Invoke-Restore @('--source', $env:SystemRoot) }
        }
        Test-Step 'journal list (restore)' {
            $last = @((Get-Doc (Invoke-Cli 'journal', 'list') 'journals.v1').jobs)[-1]
            if ($last.op -ne 'health.restore' -or $last.status -ne 'completed') { throw ($last | ConvertTo-Json -Compress) }
            "last job: $($last.op), $($last.status)"
        }
        Test-Step 'health check after restore' {
            $d = Get-Doc (Invoke-Cli 'health', 'check') 'health.v1'
            $want = if ($script:repaired) { 'healthy' } else { $script:scanState }
            if ($want -and $d.state -ne $want) { throw "state $($d.state), expected $want" }
            "$($d.state) (repaired=$script:repaired, scan said $script:scanState)"
        }
    }
    Complete-Run 'health'
}

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

Complete-Run 'smoke'
