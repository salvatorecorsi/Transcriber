param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$workspaceDirectory = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$distributionDirectory = Join-Path $workspaceDirectory 'dist'
$packageDirectory = Join-Path $distributionDirectory 'Transcriber-Windows-x64'

function Copy-VerifiedAsset($Source, $Destination, $ExpectedHash) {
    if (!(Test-Path -LiteralPath $Source -PathType Leaf)) {
        throw "Missing distribution asset: $Source"
    }
    $assetHash = (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($assetHash -ne $ExpectedHash) {
        throw "Distribution asset checksum mismatch: $Source"
    }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

if (!$SkipBuild) {
    & (Join-Path $PSScriptRoot 'build.ps1')
}

$applicationPath = Join-Path $workspaceDirectory 'Transcriber.exe'
if (!(Test-Path -LiteralPath $applicationPath -PathType Leaf)) {
    throw 'Build Transcriber.exe before packaging'
}

New-Item -ItemType Directory -Force -Path $distributionDirectory | Out-Null
$resolvedPackageDirectory = [IO.Path]::GetFullPath($packageDirectory)
$expectedPackageDirectory = [IO.Path]::GetFullPath((Join-Path $distributionDirectory 'Transcriber-Windows-x64'))
if ($resolvedPackageDirectory -ne $expectedPackageDirectory -or
    !($resolvedPackageDirectory.StartsWith("$distributionDirectory\", [StringComparison]::OrdinalIgnoreCase))) {
    throw 'Invalid distribution staging directory'
}
if (Test-Path -LiteralPath $resolvedPackageDirectory) {
    Remove-Item -LiteralPath $resolvedPackageDirectory -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $packageDirectory,
    (Join-Path $packageDirectory 'engine/bin'),
    (Join-Path $packageDirectory 'engine/bin-vulkan'),
    (Join-Path $packageDirectory 'models'),
    (Join-Path $packageDirectory 'licenses'),
    (Join-Path $packageDirectory 'audio') | Out-Null

Copy-Item -LiteralPath $applicationPath -Destination (Join-Path $distributionDirectory 'Transcriber.exe') -Force
Copy-Item -LiteralPath $applicationPath -Destination (Join-Path $packageDirectory 'Transcriber.exe') -Force
Copy-VerifiedAsset (Join-Path $workspaceDirectory 'engine/bin/corsivo-transcription.exe') `
    (Join-Path $distributionDirectory 'transcriber-ai-cpu.exe') `
    'e6adf6f7a62013b0fc7a499a5b10a57a0d76040767be4dd3d76094937f411d6c'
Copy-VerifiedAsset (Join-Path $workspaceDirectory 'engine/bin-vulkan/corsivo-transcription.exe') `
    (Join-Path $distributionDirectory 'transcriber-ai-vulkan.exe') `
    '51c607747cc551a1eae46b17a1c0fd878df21f97ddf0b33d67d3261c5801c6dc'
Copy-VerifiedAsset (Join-Path $workspaceDirectory 'models/muscriptor-small-f16.gguf') `
    (Join-Path $packageDirectory 'models/muscriptor-small-f16.gguf') `
    '925f55af65a20ebc4f8b45ceaf095a12b72493d436cb112623cd0041a1af23d4'
Copy-Item -LiteralPath (Join-Path $distributionDirectory 'transcriber-ai-cpu.exe') `
    -Destination (Join-Path $packageDirectory 'engine/bin/transcriber-ai.exe') -Force
Copy-Item -LiteralPath (Join-Path $distributionDirectory 'transcriber-ai-vulkan.exe') `
    -Destination (Join-Path $packageDirectory 'engine/bin-vulkan/transcriber-ai.exe') -Force

Get-ChildItem -LiteralPath (Join-Path $workspaceDirectory 'assets/licenses') -File |
    ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $packageDirectory 'licenses') }
Copy-Item -LiteralPath (Join-Path $workspaceDirectory 'assets/fonts/OFL-Petaluma.txt') `
    -Destination (Join-Path $packageDirectory 'licenses/OFL-Petaluma.txt')
Copy-Item -LiteralPath (Join-Path $workspaceDirectory 'assets/sounds/LICENSE-GeneralUser.txt') `
    -Destination (Join-Path $packageDirectory 'licenses/LICENSE-GeneralUser.txt')
if (Test-Path -LiteralPath (Join-Path $workspaceDirectory 'LICENSE') -PathType Leaf) {
    Copy-Item -LiteralPath (Join-Path $workspaceDirectory 'LICENSE') -Destination (Join-Path $packageDirectory 'LICENSE.txt')
}
Copy-Item -LiteralPath (Join-Path $workspaceDirectory 'README.md') -Destination (Join-Path $packageDirectory 'README.md')
Set-Content -LiteralPath (Join-Path $packageDirectory 'portable.flag') -Value '' -Encoding ascii
@'
Transcriber! - Windows x64

Estrai tutta la cartella e apri Transcriber.exe.
Il motore AI e il modello sono gia inclusi: non servono installazioni, account o connessione.
Font e suoni MIDI sono incorporati nell'applicazione.

Apri il tuo audio con l'icona della cartella e scegli Trascrivi audio dal menu in alto a destra.
Seleziona la parte da studiare e usa le icone Spartito / Tablatura e Audio / MIDI.
Clicca una nota per ascoltarla; trascina le due stanghette colorate per regolare il loop.
Il menu contiene gli export MIDI e MusicXML e le finestre Modalita strumenti e Impostazioni di lettura.
Le trascrizioni e le impostazioni si salvano nella stessa cartella dell'app.

Se scarichi soltanto Transcriber.exe, aprilo e scegli Installa AI dal menu una volta.
In questa modalita i componenti AI e i salvataggi si trovano in %LOCALAPPDATA%\Transcriber.

Motore muscriptor.cpp: MIT. Modello MuScriptor small: CC BY-NC 4.0.
Attribuzioni e condizioni d'uso complete sono nella cartella licenses.

https://github.com/salvatorecorsi/Transcriber
'@ | Set-Content -LiteralPath (Join-Path $packageDirectory 'LEGGIMI.txt') -Encoding utf8

$archivePath = Join-Path $distributionDirectory 'Transcriber-Windows-x64.zip'
Compress-Archive -LiteralPath $packageDirectory -DestinationPath $archivePath -CompressionLevel Optimal -Force
$releaseAssets = @('Transcriber.exe', 'Transcriber-Windows-x64.zip', 'transcriber-ai-cpu.exe', 'transcriber-ai-vulkan.exe')
$checksums = $releaseAssets | ForEach-Object {
    $assetPath = Join-Path $distributionDirectory $_
    '{0}  {1}' -f (Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant(), $_
}
$checksums | Set-Content -LiteralPath (Join-Path $distributionDirectory 'SHA256SUMS') -Encoding ascii
Get-Item -LiteralPath ($releaseAssets | ForEach-Object { Join-Path $distributionDirectory $_ }) |
    Select-Object Name, Length
