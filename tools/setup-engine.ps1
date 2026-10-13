param([switch]$Vulkan)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$workspace = Split-Path $PSScriptRoot -Parent
$engineDirectory = Join-Path $workspace 'engine'
$toolchainDirectory = Join-Path $engineDirectory 'toolchain'
$vendorDirectory = Join-Path $engineDirectory 'vendor'
$modelDirectory = Join-Path $workspace 'models'
$sourceCommit = 'c8adc019677ed3479fa3c22c1a117d662382f347'

function Get-VerifiedDownload($Url, $Destination, $Checksum) {
    if (Test-Path -LiteralPath $Destination) {
        if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Checksum) {
            return
        }
        throw "Checksum mismatch: $Destination"
    }
    $partialDownload = "$Destination.part"
    Invoke-WebRequest -Uri $Url -OutFile $partialDownload
    if ((Get-FileHash -LiteralPath $partialDownload -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Checksum) {
        throw "Checksum mismatch: $partialDownload"
    }
    Move-Item -LiteralPath $partialDownload -Destination $Destination
}

New-Item -ItemType Directory -Force -Path $engineDirectory, $toolchainDirectory, $modelDirectory | Out-Null
if (!(Test-Path -LiteralPath (Join-Path $vendorDirectory '.git'))) {
    git clone https://github.com/DamRsn/muscriptor.cpp.git $vendorDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Cannot download muscriptor.cpp' }
}
git -C $vendorDirectory checkout --detach $sourceCommit
if ($LASTEXITCODE -ne 0) { throw 'Cannot select pinned muscriptor.cpp commit' }

$compilerDirectory = Join-Path $toolchainDirectory 'llvm-mingw-20261006-ucrt-x86_64'
if (!(Test-Path -LiteralPath (Join-Path $compilerDirectory 'bin/clang++.exe'))) {
    $compilerArchive = Join-Path $toolchainDirectory 'llvm.zip'
    Get-VerifiedDownload 'https://github.com/mstorsjo/llvm-mingw/releases/download/20261006/llvm-mingw-20261006-ucrt-x86_64.zip' $compilerArchive '317492c456aa27ee607a5919f1d2d38dcdc1112516a24d0bf4b00d078f52d17a'
    Expand-Archive -LiteralPath $compilerArchive -DestinationPath $toolchainDirectory -Force
}

$cmakeDirectory = Join-Path $toolchainDirectory 'cmake-3.31.8-windows-x86_64'
if (!(Test-Path -LiteralPath (Join-Path $cmakeDirectory 'bin/cmake.exe'))) {
    $cmakeArchive = Join-Path $toolchainDirectory 'cmake.zip'
    Get-VerifiedDownload 'https://github.com/Kitware/CMake/releases/download/v3.31.8/cmake-3.31.8-windows-x86_64.zip' $cmakeArchive '81aa9964dbabd71fe02e7ec50472fd3ad56138c49944515ece9001efbff8d719'
    Expand-Archive -LiteralPath $cmakeArchive -DestinationPath $toolchainDirectory -Force
}

$ninjaDirectory = Join-Path $toolchainDirectory 'ninja'
if (!(Test-Path -LiteralPath (Join-Path $ninjaDirectory 'ninja.exe'))) {
    $ninjaArchive = Join-Path $toolchainDirectory 'ninja.zip'
    Get-VerifiedDownload 'https://github.com/ninja-build/ninja/releases/download/v1.13.1/ninja-win.zip' $ninjaArchive '26a40fa8595694dec2fad4911e62d29e10525d2133c9a4230b66397774ae25bf'
    Expand-Archive -LiteralPath $ninjaArchive -DestinationPath $ninjaDirectory -Force
}

$modelPath = Join-Path $modelDirectory 'muscriptor-small-f16.gguf'
Get-VerifiedDownload 'https://huggingface.co/DamRsn/muscriptor-gguf/resolve/main/v1/muscriptor-small-f16.gguf' $modelPath '925f55af65a20ebc4f8b45ceaf095a12b72493d436cb112623cd0041a1af23d4'

$env:PATH = "$(Join-Path $compilerDirectory 'bin');$ninjaDirectory;$env:PATH"
$env:CC = Join-Path $compilerDirectory 'bin/clang.exe'
$env:CXX = Join-Path $compilerDirectory 'bin/clang++.exe'
$env:RC = Join-Path $compilerDirectory 'bin/llvm-windres.exe'
$cmakeExecutable = Join-Path $cmakeDirectory 'bin/cmake.exe'
$buildDirectory = Join-Path $engineDirectory $(if ($Vulkan) { 'build-vulkan' } else { 'build' })
$outputDirectory = Join-Path $engineDirectory $(if ($Vulkan) { 'bin-vulkan' } else { 'bin' })
$vulkanEnabled = if ($Vulkan) { 'ON' } else { 'OFF' }
$configureArguments = @('-S', $engineDirectory, '-B', $buildDirectory, '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=Release', "-DMUSCRIPTOR_VULKAN=$vulkanEnabled", '-DMUSCRIPTOR_NATIVE=OFF', "-DCORSIVO_ENGINE_OUTPUT=$outputDirectory")

if ($Vulkan) {
    $sevenZip = Join-Path $env:ProgramFiles '7-Zip/7z.exe'
    if (!(Test-Path -LiteralPath $sevenZip)) {
        throw 'Install 7-Zip to extract the local Vulkan build tools, or run without -Vulkan for CPU'
    }
    $sdkDirectory = Join-Path $toolchainDirectory 'vulkan-sdk'
    $sdkArchive = Join-Path $toolchainDirectory 'vulkansdk.exe'
    if (!(Test-Path -LiteralPath (Join-Path $sdkDirectory 'Bin/glslc.exe'))) {
        Get-VerifiedDownload 'https://sdk.lunarg.com/sdk/download/1.4.363.0/windows/vulkansdk-windows-X64-1.4.363.0.exe' $sdkArchive '94a82d378f7a5e3e54c9db7d2fb7016af136e14ac0a18dbf0f2f67a36352d141'
        & $sevenZip x $sdkArchive "-o$sdkDirectory" 'Bin/glslc.exe' -y
        if ($LASTEXITCODE -gt 1) { throw 'Cannot extract Vulkan shader compiler' }
    }
    $headerDirectory = Join-Path $toolchainDirectory 'Vulkan-Headers-1.4.363'
    if (!(Test-Path -LiteralPath (Join-Path $headerDirectory 'include/vulkan/vulkan.h'))) {
        $headerArchive = Join-Path $toolchainDirectory 'vulkan-headers.zip'
        Get-VerifiedDownload 'https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/v1.4.363.zip' $headerArchive '112c94f35e6ff2f85d4b92b50d0611883a6467dab202a2547312defb87351c16'
        Expand-Archive -LiteralPath $headerArchive -DestinationPath $toolchainDirectory -Force
    }
    $spirvDirectory = Join-Path $toolchainDirectory 'SPIRV-Headers-vulkan-sdk-1.4.341.0'
    if (!(Test-Path -LiteralPath (Join-Path $spirvDirectory 'CMakeLists.txt'))) {
        $spirvArchive = Join-Path $toolchainDirectory 'spirv-headers.zip'
        Get-VerifiedDownload 'https://github.com/KhronosGroup/SPIRV-Headers/archive/refs/tags/vulkan-sdk-1.4.341.0.zip' $spirvArchive 'b7cbda302ef5f3d60ad443d456eaf5c610bc7ff99439d519cecd2b50d9e7d9f2'
        Expand-Archive -LiteralPath $spirvArchive -DestinationPath $toolchainDirectory -Force
    }
    $env:VULKAN_SDK = $sdkDirectory
    $spirvBuild = Join-Path $toolchainDirectory 'spirv-build'
    & $cmakeExecutable -S $spirvDirectory -B $spirvBuild -G Ninja -DSPIRV_HEADERS_ENABLE_TESTS=OFF "-DCMAKE_INSTALL_PREFIX=$sdkDirectory"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot configure SPIRV headers' }
    & $cmakeExecutable --install $spirvBuild
    if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare SPIRV headers' }

    $loaderLibrary = Join-Path $sdkDirectory 'Lib/libvulkan-1.a'
    if (!(Test-Path -LiteralPath $loaderLibrary)) {
        New-Item -ItemType Directory -Force -Path (Join-Path $sdkDirectory 'Lib') | Out-Null
        $loaderPath = Join-Path $env:WINDIR 'System32/vulkan-1.dll'
        if (!(Test-Path -LiteralPath $loaderPath)) { throw 'The graphics driver must provide vulkan-1.dll' }
        $loaderExports = & (Join-Path $compilerDirectory 'bin/llvm-readobj.exe') --coff-exports $loaderPath
        $loaderSymbols = $loaderExports | Where-Object { $_ -match '^  Name: ' } | ForEach-Object { $_.Substring(8) }
        $loaderDefinition = Join-Path $sdkDirectory 'Lib/vulkan-1.def'
        @('LIBRARY vulkan-1.dll', 'EXPORTS') + $loaderSymbols | Set-Content -Encoding ascii -LiteralPath $loaderDefinition
        & (Join-Path $compilerDirectory 'bin/llvm-dlltool.exe') -m i386:x86-64 -d $loaderDefinition -l $loaderLibrary
        if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare Vulkan import library' }
    }
    $configureArguments += @("-DVulkan_INCLUDE_DIR=$(Join-Path $headerDirectory 'include')", "-DVulkan_LIBRARY=$loaderLibrary", "-DVulkan_GLSLC_EXECUTABLE=$(Join-Path $sdkDirectory 'Bin/glslc.exe')")
    $existingGgml = Join-Path $engineDirectory 'build/_deps/ggml-src'
    if (Test-Path -LiteralPath (Join-Path $existingGgml 'CMakeLists.txt')) {
        $configureArguments += "-DFETCHCONTENT_SOURCE_DIR_GGML=$existingGgml"
    }
}

& $cmakeExecutable @configureArguments
if ($LASTEXITCODE -ne 0) { throw 'Cannot configure transcription engine' }
& $cmakeExecutable --build $buildDirectory --parallel 6
if ($LASTEXITCODE -ne 0) { throw 'Cannot build transcription engine' }
Write-Host "Transcription engine ready: $(Join-Path $outputDirectory 'corsivo-transcription.exe')"
