<#
What CI takes from the Windows ADK (decision #22). Microsoft's installers come from the fwlinks on
learn.microsoft.com/windows-hardware/get-started/adk-install (ADK_URL, WINPE_URL) and must carry a
valid Microsoft signature before they run. No file of the kit or the SDK leaves the job: they
aren't redistributable, and a public repository's artifacts and caches can be read by others.

  -Check <exe>    the S-03 header cross-check. <exe> is dpro-sys's ffi-check-source example (from
                  t3-tools); it prints the C static asserts for each header, and clang-cl /Zs
                  compiles them for x64 and ARM64 against dismapi.h (ADK Deployment Tools) and
                  wimgapi.h (the Windows SDK on the runner). Only pass or fail leaves the job.
                  `cargo xtask ffi-check` is the same check for a local run.
  -WinPE <dir>    <arch>.txt per architecture: the DLL names in base WinPE's System32 (winpe.wim, no
                  optional components), the evidence behind xtask/data/winpe-dlls.txt
#>
param([string] $Check, [string] $WinPE)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$kits = "${env:ProgramFiles(x86)}\Windows Kits\10"
$adk = Join-Path $kits 'Assessment and Deployment Kit'
$peRoot = Join-Path $adk 'Windows Preinstallation Environment'

function Find-File([string] $Dir, [string] $Name) {
    Get-ChildItem -Path $Dir -Recurse -Filter $Name -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1
}

# Downloads a kit installer, checks its signature, installs one feature quietly.
function Install-Kit([string] $Url, [string] $Feature, [string] $Name) {
    $setup = Join-Path $env:RUNNER_TEMP "$Name.exe"
    Invoke-WebRequest -Uri $Url -OutFile $setup
    $sig = Get-AuthenticodeSignature $setup
    if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch '^CN=Microsoft Corporation,') {
        throw "${Name}.exe signature: $($sig.Status), $($sig.SignerCertificate.Subject)"
    }
    $log = Join-Path $env:RUNNER_TEMP "$Name.log"
    $p = Start-Process -FilePath $setup -Wait -PassThru -ArgumentList `
        '/quiet', '/norestart', '/ceip', 'off', '/features', $Feature, '/log', $log
    Write-Host "${Name}.exe exit code: $($p.ExitCode)"
}

function Show-Logs([string] $Name) {
    Get-ChildItem "$env:RUNNER_TEMP\$Name*.log" -ErrorAction SilentlyContinue |
        ForEach-Object { Write-Host "--- $($_.Name)"; Get-Content $_ -Tail 60 }
}

function Install-DeploymentTools {
    if (Find-File $adk dismapi.h) { return }
    Install-Kit $env:ADK_URL 'OptionId.DeploymentTools' 'adksetup'
    if (-not (Find-File $adk dismapi.h)) {
        Show-Logs 'adksetup'
        throw "dismapi.h not found under $adk after installing the ADK"
    }
}

# clang-cl from LLVM, else the one in Visual Studio's Clang component.
function Find-ClangCl {
    $cmd = Get-Command clang-cl -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $llvm = Join-Path $env:ProgramFiles 'LLVM\bin\clang-cl.exe'
    if (Test-Path $llvm) { return $llvm }
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $vs = & $vswhere -latest -property installationPath
    $bundled = Join-Path "$vs" 'VC\Tools\Llvm\x64\bin\clang-cl.exe'
    if (Test-Path $bundled) { return $bundled }
    throw 'clang-cl not found (LLVM, or the Clang component of Visual Studio)'
}

# GitHub annotation text: % and line breaks escaped.
function Format-Annotation([string] $Text) {
    $Text.Replace('%', '%25').Replace("`r", '%0D').Replace("`n", '%0A')
}

# The compiler's error lines (paths trimmed to the file name), capped like xtask's.
function Select-Errors([string[]] $Lines) {
    $errors = foreach ($line in $Lines) {
        $at = $line.IndexOf('error:')
        if ($at -ge 0) { (($line.Substring(0, $at) -split '[\\/]')[-1]) + $line.Substring($at) }
    }
    if (-not $errors) { return ($Lines | Select-Object -Last 10) -join "`n" }
    $text = ($errors | Select-Object -First 40) -join "`n"
    if (@($errors).Count -gt 40) { $text += "`n… $(@($errors).Count - 40) more" }
    $text
}

if ($Check) {
    Install-DeploymentTools
    $dism = Find-File $adk dismapi.h
    # wimgapi.h ships with the Windows SDK (installed on the runner), not the ADK.
    $wim = (Find-File $adk wimgapi.h) ?? (Find-File (Join-Path $kits Include) wimgapi.h)
    if (-not $wim) { throw "wimgapi.h not found under $kits" }
    $generator = (Resolve-Path $Check).Path
    $clang = Find-ClangCl
    & $clang --version | Out-Host
    # The ADK and SDK dirs as system includes; clang-cl finds the rest of the SDK and the MSVC
    # headers on the runner itself.
    $includes = '/imsvc', $dism.DirectoryName, '/imsvc', $wim.DirectoryName
    $work = Join-Path $env:RUNNER_TEMP 'ffi-check'
    $null = New-Item -ItemType Directory -Force $work
    $failures = @()
    foreach ($header in 'dismapi.h', 'wimgapi.h') {
        $source = Join-Path $work "$header.c"
        $text = & $generator $header | Out-String
        if ($LASTEXITCODE) { throw "$generator $header exited with $LASTEXITCODE" }
        [IO.File]::WriteAllText($source, $text)
        foreach ($target in 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc') {
            $lines = & $clang /nologo /Zs /TC -Wno-unused-command-line-argument "--target=$target" `
                @includes $source 2>&1 | ForEach-Object { "$_" }
            $lines | Out-Host
            if ($LASTEXITCODE) {
                $message = Format-Annotation (Select-Errors $lines)
                Write-Host "::error title=S-03 $header ($target)::$message"
                $failures += "$header ($target)"
            } else {
                Write-Host "ffi-check: ok   $header ($target)"
            }
        }
    }
    if ($failures) { throw "$($failures.Count) ffi-check failure(s) (S-03): $($failures -join ', ')" }
}

if ($WinPE) {
    Install-DeploymentTools # the WinPE add-on installs into an existing ADK
    if (-not (Find-File $peRoot winpe.wim)) {
        Install-Kit $env:WINPE_URL 'OptionId.WindowsPreinstallationEnvironment' 'adkwinpesetup'
    }
    $null = New-Item -ItemType Directory -Force $WinPE
    foreach ($arch in 'amd64', 'arm64') {
        $image = Join-Path $peRoot "$arch\en-us\winpe.wim"
        if (-not (Test-Path $image)) {
            Show-Logs 'adkwinpesetup'
            throw "$image not found after installing the WinPE add-on"
        }
        $mount = Join-Path $env:RUNNER_TEMP "winpe-$arch"
        $null = New-Item -ItemType Directory -Force $mount
        dism /English /Mount-Image "/ImageFile:$image" /Index:1 "/MountDir:$mount" /ReadOnly | Out-Host
        if ($LASTEXITCODE) { throw "mounting $image failed: $LASTEXITCODE" }
        try {
            $dlls = Get-ChildItem (Join-Path $mount 'Windows\System32') -Filter *.dll -File |
                ForEach-Object { $_.Name.ToLowerInvariant() } | Sort-Object
            $dlls | Set-Content (Join-Path $WinPE "$arch.txt")
            Write-Host "${arch}: $(@($dlls).Count) DLLs in base WinPE System32"
        } finally {
            dism /English /Unmount-Image "/MountDir:$mount" /Discard | Out-Host
        }
    }
}
