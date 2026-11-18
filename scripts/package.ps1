# Build a portable host-target ZIP. Only the build machine needs Rust.
[CmdletBinding()]
param([switch]$Offline)
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    $target = ((rustc -vV | Select-String '^host: ').Line -replace '^host: ', '')
    if (!$target) { throw 'Cannot identify Rust host target' }
    $flags = @()
    if ($Offline) { $flags += '--offline' }
    cargo build --locked --release --target $target @flags
    if ($LASTEXITCODE) { throw 'Release build failed' }
    $metadata = cargo metadata --no-deps --format-version 1 --locked @flags | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'Cannot locate build directory' }
    $exe = if ($target -match 'windows') { 'injector.exe' } else { 'injector' }
    $stage = Join-Path ([IO.Path]::GetTempPath()) ("injector-package-" + [guid]::NewGuid())
    $null = New-Item "$stage/defs" -ItemType Directory -Force
    try {
        Copy-Item "$($metadata.target_directory)/$target/release/$exe" $stage
        # Distribute the committed template, never a locally edited key.
        $example = git show HEAD:defs/example.toml
        if ($LASTEXITCODE) { throw 'Cannot read committed example definition' }
        [IO.File]::WriteAllLines("$stage/defs/example.toml", [string[]]$example)
        Copy-Item README.md $stage
        $null = New-Item dist -ItemType Directory -Force
        $zip = "dist/agent-config-injector-$target.zip"
        Compress-Archive -Path "$stage/*" -DestinationPath $zip -Force
        Get-Item $zip | Select-Object FullName, Length
    } finally { Remove-Item -LiteralPath $stage -Recurse -Force }
} finally { Pop-Location }
