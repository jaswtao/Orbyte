# Author: jaswtao · Date: 2026-10-20
param([Parameter(Mandatory)][string]$Tag)
$ErrorActionPreference = 'Stop'
$originalTargetDirectory = $env:CARGO_TARGET_DIR
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    python scripts/check-release.py --tag $Tag
    if ($LASTEXITCODE -ne 0) { throw 'Version check failed.' }
    $commit = git rev-parse --verify HEAD
    if ($LASTEXITCODE -ne 0) { throw 'A source commit is required.' }
    $tagCommit = git rev-parse --verify "refs/tags/${Tag}^{commit}"
    if ($LASTEXITCODE -ne 0) { throw "Release tag $Tag must exist before packaging." }
    if ($tagCommit -ne $commit) { throw "Release tag $Tag must point to HEAD." }
    $status = git status --porcelain --untracked-files=normal
    if ($LASTEXITCODE -ne 0) { throw 'Cannot verify source working tree.' }
    if ($status) { throw 'Commit the reviewed source before packaging a release.' }
    $metadata = cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the Cargo target directory.' }
    $targetDirectory = ($metadata | ConvertFrom-Json).target_directory
    if (-not $targetDirectory) { throw 'Cargo metadata is missing the target directory.' }
    # Keep Cargo's resolved absolute directory when Tauri changes the working directory.
    $env:CARGO_TARGET_DIR = $targetDirectory
    $installer = Join-Path $targetDirectory "release/bundle/nsis/Orbyte_$($Tag.Substring(1))_x64-setup.exe"
    $output = Join-Path (Get-Location) "release/$Tag"
    if (Test-Path -LiteralPath $output) { throw 'Release output already exists; choose a clean output directory.' }
    New-Item -ItemType Directory -Path $output | Out-Null
    python scripts/generate-licenses.py --check --mpl-sources "$output/third-party-mpl-sources.zip"
    if ($LASTEXITCODE -ne 0) { throw 'Third-party verification failed.' }
    # Remove only the expected installer so a successful build cannot reuse a stale package.
    if (Test-Path -LiteralPath $installer) { Remove-Item -LiteralPath $installer }
    pnpm build
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed.' }
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw "Expected installer was not produced: $installer" }
    Copy-Item -LiteralPath $installer -Destination $output
    Copy-Item -LiteralPath LICENSE,THIRD_PARTY_NOTICES.txt,THIRD_PARTY_LICENSES.json -Destination $output
    git archive --format=zip "--prefix=Orbyte-$($Tag.TrimStart('v'))/" "--output=$output/Orbyte-$Tag-source.zip" $commit
    if ($LASTEXITCODE -ne 0) { throw 'Source archive failed.' }
    $hashes = Get-ChildItem -LiteralPath $output -File | Sort-Object Name | ForEach-Object {
        "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"
    }
    $hashes | Set-Content -LiteralPath "$output/SHA256SUMS.txt" -Encoding utf8
    Write-Output "Release artifacts: $output"
} finally {
    $env:CARGO_TARGET_DIR = $originalTargetDirectory
    Pop-Location
}
