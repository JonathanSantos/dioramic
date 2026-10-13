# Installs Dioramic from the GitHub releases on Windows.
#
#   irm https://github.com/JonathanSantos/dioramic/releases/latest/download/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://github.com/JonathanSantos/dioramic/releases/latest/download/install.ps1))) -Desktop
#
# Parameters (each also has an environment variable, handy with `irm | iex`, which cannot pass parameters):
#   -Desktop           install the desktop app instead of the CLI              (DIORAMIC_DESKTOP=1)
#   -Version <tag>     a release tag such as v0.1.0; default: latest           (DIORAMIC_VERSION)
#   -InstallDir <dir>  where the CLI goes; default: %LOCALAPPDATA%\Programs\dioramic\bin  (DIORAMIC_INSTALL_DIR)
#   -DryRun            print what would be downloaded and exit
#
# Asset names must stay in sync with .github/workflows/release.yml.
param(
    [switch]$Desktop = ($env:DIORAMIC_DESKTOP -eq '1'),
    [string]$Version = $(if ($env:DIORAMIC_VERSION) { $env:DIORAMIC_VERSION } else { 'latest' }),
    [string]$InstallDir = $(if ($env:DIORAMIC_INSTALL_DIR) { $env:DIORAMIC_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\dioramic\bin' }),
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
# Invoke-WebRequest's progress bar slows downloads down by an order of magnitude on Windows PowerShell 5.1.
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Repo = if ($env:DIORAMIC_REPO) { $env:DIORAMIC_REPO } else { 'JonathanSantos/dioramic' }

function Write-Step([string]$Message) { Write-Host "dioramic: $Message" }

function Get-ReleaseUrl {
    if ($Version -eq 'latest') { return "https://github.com/$Repo/releases/latest/download" }
    $tag = if ($Version.StartsWith('v')) { $Version } else { "v$Version" }
    return "https://github.com/$Repo/releases/download/$tag"
}

function Get-Arch {
    $arch = $null
    try { $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch { }
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    switch -Regex ($arch) {
        '^(X64|AMD64)$' { return 'x86_64' }
        '^ARM64$' { return 'aarch64' }
        default { throw "dioramic: unsupported CPU architecture $arch" }
    }
}

# Downloads an asset and checks it against the `<asset>.sha256` published next to it.
function Get-VerifiedAsset([string]$Url, [string]$Destination) {
    Write-Step "downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing
    Invoke-WebRequest -Uri "$Url.sha256" -OutFile "$Destination.sha256" -UseBasicParsing
    $expected = ((Get-Content "$Destination.sha256" -Raw).Trim() -split '\s+')[0]
    $actual = (Get-FileHash -Algorithm SHA256 $Destination).Hash
    if ($expected -ne $actual) { throw "dioramic: checksum mismatch for $(Split-Path $Destination -Leaf)" }
}

function Add-ToUserPath([string]$Directory) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @($userPath -split ';' | Where-Object { $_ })
    if ($entries -contains $Directory) { return }
    [Environment]::SetEnvironmentVariable('Path', (($entries + $Directory) -join ';'), 'User')
    $env:Path = "$env:Path;$Directory"
    Write-Step "added $Directory to your user PATH (open a new terminal to pick it up)"
}

function Install-Cli([string]$WorkDir) {
    $target = "$(Get-Arch)-pc-windows-msvc"
    $asset = "dioramic-cli-$target.zip"
    $url = "$(Get-ReleaseUrl)/$asset"
    if ($DryRun) {
        Write-Step "would download $url"
        Write-Step "would install $(Join-Path $InstallDir 'dioramic.exe')"
        return
    }
    $archive = Join-Path $WorkDir $asset
    Get-VerifiedAsset $url $archive
    Expand-Archive -Path $archive -DestinationPath $WorkDir -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $binary = Join-Path $InstallDir 'dioramic.exe'
    Copy-Item (Join-Path $WorkDir "dioramic-cli-$target\dioramic.exe") $binary -Force
    Write-Step "installed $binary ($(& $binary --version))"
    Add-ToUserPath $InstallDir
    Write-Step "next: run 'dioramic init' in your project to register the MCP server with your AI clients"
}

function Install-Desktop([string]$WorkDir) {
    # Only an x64 build exists; Windows on ARM runs it under emulation.
    $asset = 'dioramic-desktop-windows-x86_64-setup.exe'
    $url = "$(Get-ReleaseUrl)/$asset"
    if ($DryRun) {
        Write-Step "would download $url and run the installer"
        return
    }
    $installer = Join-Path $WorkDir $asset
    Get-VerifiedAsset $url $installer
    Write-Step 'running the installer'
    Start-Process -FilePath $installer -Wait
    Write-Step 'done; Dioramic is in the Start menu'
}

$workDir = Join-Path ([IO.Path]::GetTempPath()) "dioramic-install-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
try {
    if ($Desktop) { Install-Desktop $workDir } else { Install-Cli $workDir }
}
finally {
    Remove-Item -Recurse -Force $workDir -ErrorAction SilentlyContinue
}
