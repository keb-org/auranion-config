$ErrorActionPreference = "Stop"

$Repo = "keb-org/auranion-config"
$InstallDir = "$env:LOCALAPPDATA\Programs\auranion"
$ExePath = "$InstallDir\auranion.exe"

$ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
$Release = Invoke-RestMethod -Uri $ReleaseUrl -Headers @{ "User-Agent" = "auranion-installer" }
$Asset = $Release.assets | Where-Object { $_.name -eq "auranion-windows-amd64.exe" }

if (-not $Asset) {
    throw "No Windows release binary found."
}

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

Write-Host "Downloading Auranion CLI..."
Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $ExePath

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path += ";$InstallDir"
    Write-Host "Added $InstallDir to PATH."
}

Write-Host "Auranion CLI installed successfully! Run 'auranion config' to start."
