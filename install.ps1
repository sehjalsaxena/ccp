# Windows PowerShell installation script for ccp
[CmdletBinding()]
param(
    [string]$Version = $env:CCP_VERSION,
    [string]$Repo = $(if ($env:CCP_REPO) { $env:CCP_REPO } else { "smresponsibilities/ccp" }),
    [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { "$env:USERPROFILE\.ccp\bin" })
)

$ErrorActionPreference = 'Stop'

$BinaryName = "ccp.exe"
$AssetName = "ccp-windows-amd64.zip"

if ($Version) {
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$AssetName"
} else {
    $DownloadUrl = "https://github.com/$Repo/releases/latest/download/$AssetName"
}

Write-Host "==> Installing ccp (windows/amd64)..." -ForegroundColor Cyan
Write-Host "==> Downloading from: $DownloadUrl" -ForegroundColor Cyan

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ccp-install-" + [System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
$ArchiveFile = Join-Path $TempDir $AssetName

try {
    # Ensure TLS 1.2 and TLS 1.3 are enabled
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArchiveFile -UseBasicParsing
    
    if (-not (Test-Path $ArchiveFile)) {
        Write-Error "[ERROR] Download failed: archive file was not found."
        exit 1
    }

    Expand-Archive -Path $ArchiveFile -DestinationPath $TempDir -Force

    # Locate ccp.exe inside extracted files
    $ExtractedExe = Get-ChildItem -Path $TempDir -Filter $BinaryName -Recurse -File | Select-Object -First 1

    if (-not $ExtractedExe) {
        Write-Error "[ERROR] Could not find $BinaryName in downloaded archive."
        exit 1
    }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $TargetExe = Join-Path $InstallDir $BinaryName
    Copy-Item -Path $ExtractedExe.FullName -Destination $TargetExe -Force

    Write-Host "[OK] ccp installed to $TargetExe" -ForegroundColor Green

    # Check and update User PATH
    $UserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
    $PathParts = ($UserPath -split ';') | Where-Object { $_ -ne '' }
    $NormalizedInstallDir = $InstallDir.TrimEnd('\')
    $AlreadyInPath = $PathParts | Where-Object { $_.TrimEnd('\') -ieq $NormalizedInstallDir }

    if (-not $AlreadyInPath) {
        Write-Host ""
        Write-Host "[WARN] $InstallDir is not in your User PATH." -ForegroundColor Yellow
        Write-Host "Adding $InstallDir to your User PATH..." -ForegroundColor Cyan

        $NewPath = if ([string]::IsNullOrWhiteSpace($UserPath)) { $InstallDir } else { "$UserPath;$InstallDir" }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, [EnvironmentVariableTarget]::User)
        $env:PATH = "$env:PATH;$InstallDir"

        Write-Host "[OK] Added $InstallDir to User PATH." -ForegroundColor Green
        Write-Host "Please restart your terminal or PowerShell window for PATH changes to take effect." -ForegroundColor Yellow
    }

    Write-Host ""
    Write-Host "Run 'ccp set' to configure credentials or 'ccp' to log in." -ForegroundColor Cyan
}
finally {
    if (Test-Path $TempDir) {
        Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
