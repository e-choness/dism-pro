<#
What CI takes from the Windows ADK (decision #22). Microsoft's installers come from the fwlinks on
learn.microsoft.com/windows-hardware/get-started/adk-install (ADK_URL, WINPE_URL) and must carry a
valid Microsoft signature before they run. Only derived files leave the job, never the kit itself.

  -Headers <dir>  dismapi.h (ADK Deployment Tools) and wimgapi.h (Windows SDK on the runner), for S-03
  -WinPE <dir>    <arch>.txt per architecture: the DLL names in base WinPE's System32 (winpe.wim, no
                  optional components), the evidence behind xtask/data/winpe-dlls.txt
#>
param([string] $Headers, [string] $WinPE)

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

if ($Headers) {
    Install-DeploymentTools
    $dism = Find-File $adk dismapi.h
    # wimgapi.h ships with the Windows SDK (installed on the runner), not the ADK.
    $wim = (Find-File $adk wimgapi.h) ?? (Find-File (Join-Path $kits Include) wimgapi.h)
    if (-not $wim) { throw "wimgapi.h not found under $kits" }
    $null = New-Item -ItemType Directory -Force $Headers
    Copy-Item (Join-Path $dism.DirectoryName '*.h'), $wim.FullName $Headers
    Get-ChildItem $Headers | Format-Table Name, Length
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
