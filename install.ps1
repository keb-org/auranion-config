$ErrorActionPreference = "Stop"

$Repo = "keb-org/auranion-config"
$InstallDir = "$env:LOCALAPPDATA\Programs\auranion"
$ExePath = "$InstallDir\auranion.exe"

function Get-TargetAsset {
    $arch = $null

    try {
        if (-not ([System.Management.Automation.PSTypeName]'Win32.NativeKernelArch').Type) {
            Add-Type -Namespace Win32 -Name NativeKernelArch -MemberDefinition @"
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
public static extern bool IsWow64Process2(System.IntPtr hProcess, out ushort processMachine, out ushort nativeMachine);
[System.Runtime.InteropServices.DllImport("kernel32.dll")]
public static extern System.IntPtr GetCurrentProcess();
"@ | Out-Null
        }
        [ushort]$procMachine = 0
        [ushort]$natMachine = 0
        if ([Win32.NativeKernelArch]::IsWow64Process2([Win32.NativeKernelArch]::GetCurrentProcess(), [ref]$procMachine, [ref]$natMachine)) {
            switch ($natMachine) {
                0xAA64 { $arch = "arm64" }
                0x8664 { $arch = "amd64" }
            }
        }
    } catch {
        # Fallback on older Windows without IsWow64Process2
    }

    if (-not $arch) {
        $envArch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
        switch ($envArch) {
            "ARM64" { $arch = "arm64" }
            "AMD64" { $arch = "amd64" }
        }
    }

    switch ($arch) {
        "amd64" { return "auranion-windows-amd64.exe" }
        "arm64" { return "auranion-windows-arm64.exe" }
        default {
            throw "Unsupported Windows architecture: $(if ($arch) { $arch } else { $env:PROCESSOR_ARCHITECTURE })"
        }
    }
}

$TargetAssetName = Get-TargetAsset

$ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
$Release = Invoke-RestMethod -Uri $ReleaseUrl -Headers @{ "User-Agent" = "auranion-installer" }
$Asset = $Release.assets | Where-Object { $_.name -eq $TargetAssetName }

if (-not $Asset) {
    throw "No Windows release binary found for $TargetAssetName."
}

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

$TempName = "auranion.tmp." + [System.IO.Path]::GetRandomFileName() + ".exe"
$TempPath = Join-Path $InstallDir $TempName

try {
    Write-Host "Downloading Auranion CLI ($TargetAssetName)..."
    Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $TempPath

    $downloaded = Get-Item $TempPath -ErrorAction Stop
    if ($downloaded.Length -eq 0) {
        throw "Downloaded binary is empty."
    }

    $ver = & $TempPath --version 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Downloaded binary failed execution check with exit code $LASTEXITCODE`: $ver"
    }

    Move-Item -Path $TempPath -Destination $ExePath -Force
} finally {
    if (Test-Path $TempPath) {
        Remove-Item -Path $TempPath -Force -ErrorAction SilentlyContinue
    }
}

# Add to User PATH if missing
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    $newUserPath = if ([string]::IsNullOrEmpty($UserPath)) { $InstallDir } else { "$UserPath;$InstallDir" }
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    Write-Host "Added $InstallDir to User PATH."
}

# Always patch the current process PATH so `auranion` resolves right after
# `irm ... | iex` without requiring the user to open a new terminal.
if ($env:Path -notlike "*$InstallDir*") {
    $env:Path = "$InstallDir;$env:Path"
} elseif (-not $env:Path.StartsWith($InstallDir)) {
    $env:Path = $env:Path -replace [regex]::Escape(";$InstallDir"), ""
    $env:Path = "$InstallDir;$env:Path"
}

# Broadcast WM_SETTINGCHANGE so already-open Explorers/terminals that listen
# for env changes (and Windows itself) reload the User PATH immediately.
try {
    if (-not ([System.Management.Automation.PSTypeName]'Win32.NativeMethods').Type) {
        Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition @"
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out IntPtr lpdwResult);
"@ | Out-Null
    }
    $HWND_BROADCAST = [IntPtr]0xffff
    $WM_SETTINGCHANGE = 0x001A
    $result = [IntPtr]::Zero
    [void][Win32.NativeMethods]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [IntPtr]::Zero, "Environment", 2, 5000, [ref]$result)
} catch {
    # Non-fatal: PATH is already correct for the current process and for
    # any new shell; this only helps already-open windows.
}

try {
    $ver = & $ExePath --version 2>&1
    Write-Host "$ver installed at $ExePath"
} catch {
    Write-Warning "Installed $ExePath but could not run it: $_"
}

# Verify it actually resolves as `auranion` in this session (not just via
# $ExePath), so `irm | iex` users know to restart if needed.
try {
    $null = Get-Command auranion -ErrorAction Stop
} catch {
    Write-Warning ("Installed but 'auranion' still not on PATH in this shell - restart your terminal, or run: & `"$ExePath`" config")
}

Write-Host ""
Write-Host "Auranion CLI installed successfully! Run 'auranion config' to start."
