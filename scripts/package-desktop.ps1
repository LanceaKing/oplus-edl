$ErrorActionPreference = 'Stop'

if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') {
    throw 'Run this script in PowerShell 7 on x86_64 Windows.'
}

$Root = Split-Path -Parent $PSScriptRoot
$Target = 'x86_64-pc-windows-msvc'
$BuildDir = Join-Path $Root 'target'
$Features = 'oplus-edl-cli/usb,oplus-edl-cli/serial,oplus-edl-cli/cloud,oplus-edl-desktop/usb,oplus-edl-desktop/serial,oplus-edl-desktop/cloud'

Push-Location -LiteralPath $Root
try {
    cargo build --locked --offline --release --bins --target $Target --target-dir $BuildDir `
        -p oplus-edl-cli -p oplus-edl-desktop --no-default-features --features $Features
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed with exit code $LASTEXITCODE."
    }

    $Dist = Join-Path $Root 'dist'
    New-Item -ItemType Directory -Path $Dist -Force | Out-Null
    $Stage = Join-Path $Dist ".release.$([Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $Stage | Out-Null
    try {
        $Contents = Join-Path $Stage 'oplus-edl'
        $Binaries = Join-Path (Join-Path $BuildDir $Target) 'release'
        $Version = python scripts/desktop-release.py stage $Target $Features $Binaries $Contents
        if ($LASTEXITCODE -ne 0) {
            throw "Staging failed with exit code $LASTEXITCODE."
        }

        $Archive = "oplus-edl-$Version-windows-x86_64.zip"
        $StagedArchive = Join-Path $Stage $Archive
        Compress-Archive -LiteralPath $Contents -DestinationPath $StagedArchive
        python scripts/desktop-release.py verify $Target $StagedArchive
        if ($LASTEXITCODE -ne 0) {
            throw "Archive verification failed with exit code $LASTEXITCODE."
        }

        $Output = Join-Path $Dist $Archive
        [System.IO.File]::Move($StagedArchive, $Output, $true)
        Write-Output $Output
    } finally {
        Remove-Item -LiteralPath $Stage -Recurse -Force
    }
} finally {
    Pop-Location
}
