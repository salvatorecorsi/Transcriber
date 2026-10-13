$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
Push-Location $workspace
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Application build failed' }
    Copy-Item -LiteralPath (Join-Path $workspace 'target/release/transcriber.exe') -Destination (Join-Path $workspace 'Transcriber.exe') -Force
    Write-Host 'Transcriber.exe is ready'
} finally {
    Pop-Location
}
