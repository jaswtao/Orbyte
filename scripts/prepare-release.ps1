# Author: jaswtao · Date: 2026-10-20
param([Parameter(Mandatory)][string]$Tag)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    python scripts/check-release.py --tag $Tag
    if ($LASTEXITCODE -ne 0) { throw 'Version check failed.' }
    if (git status --porcelain --untracked-files=normal) { throw 'Commit the reviewed source before packaging a release.' }
    git rev-parse --verify HEAD | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'A source commit is required.' }
    $output = Join-Path (Get-Location) "release/$Tag"
    if (Test-Path -LiteralPath $output) { throw 'Release output already exists; choose a clean output directory.' }
    New-Item -ItemType Directory -Path $output | Out-Null
    python scripts/generate-licenses.py --check --mpl-sources "$output/third-party-mpl-sources.zip"
    if ($LASTEXITCODE -ne 0) { throw 'Third-party verification failed.' }
    pnpm build
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed.' }
    $installers = @(Get-ChildItem target/release/bundle/nsis/*-setup.exe)
    if ($installers.Count -ne 1) { throw 'Expected exactly one NSIS installer.' }
    Copy-Item -LiteralPath $installers[0].FullName -Destination $output
    Copy-Item -LiteralPath LICENSE,THIRD_PARTY_NOTICES.txt,THIRD_PARTY_LICENSES.json -Destination $output
    git archive --format=zip "--prefix=Orbyte-$($Tag.TrimStart('v'))/" "--output=$output/Orbyte-$Tag-source.zip" HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Source archive failed.' }
    $hashes = Get-ChildItem -LiteralPath $output -File | Sort-Object Name | ForEach-Object {
        "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"
    }
    $hashes | Set-Content -LiteralPath "$output/SHA256SUMS.txt" -Encoding utf8
    Write-Output "Release artifacts: $output"
} finally { Pop-Location }
