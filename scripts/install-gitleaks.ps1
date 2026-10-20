# Author: jaswtao · Date: 2026-10-20
$ErrorActionPreference = 'Stop'
$version = '8.30.1'
$directory = Join-Path ([System.IO.Path]::GetTempPath()) "orbyte-gitleaks-$version"
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$archiveName = "gitleaks_${version}_windows_x64.zip"
$base = "https://github.com/gitleaks/gitleaks/releases/download/v$version"
$archive = Join-Path $directory $archiveName
$checksums = Join-Path $directory 'checksums.txt'
Invoke-WebRequest "$base/$archiveName" -OutFile $archive
Invoke-WebRequest "$base/gitleaks_${version}_checksums.txt" -OutFile $checksums
$entry = @(Get-Content $checksums | Where-Object { ($_ -split '\s+')[-1] -eq $archiveName })
if ($entry.Count -ne 1) { throw 'Cannot find a unique Gitleaks archive checksum.' }
$expected = ($entry[0] -split '\s+')[0]
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ine $expected) { throw 'Gitleaks checksum mismatch.' }
Expand-Archive -LiteralPath $archive -DestinationPath $directory -Force
Write-Output (Join-Path $directory 'gitleaks.exe')
