# Starts or stops the isolated OpenSSH fixture used by the ignored SSH probe.
# Author: jaswtao · Date: 2026-10-31
param(
    [string]$Password = 'orbyte-probe',
    [string]$KeyPassphrase = 'orbyte-key-passphrase',
    [switch]$Down
)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
# docker compose parses the whole file even for `down`, so the variable is always required.
$env:SSH_PASSWORD = $Password
try {
    if ($Down) {
        docker compose down --remove-orphans
        if ($LASTEXITCODE -ne 0) { throw 'docker compose down failed' }
        Write-Host 'SSH probe fixture stopped.'
        return
    }

    foreach ($command in @('docker', 'ssh-keygen')) {
        if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
            throw "$command is required; see doc/ssh-probe.md"
        }
    }

    $keyDir = Join-Path $PSScriptRoot 'keys'
    $fixtureDir = Join-Path $PSScriptRoot 'fixtures'
    New-Item -ItemType Directory -Force -Path $keyDir, $fixtureDir | Out-Null

    function New-ProbeKey([string]$Name, [string[]]$Arguments) {
        $path = Join-Path $keyDir $Name
        if (Test-Path -LiteralPath $path) { return $path }
        & ssh-keygen -q @Arguments -C "orbyte-$Name" -f $path
        if ($LASTEXITCODE -ne 0) { throw "ssh-keygen failed for $Name" }
        return $path
    }

    $empty = ''
    $ed25519 = New-ProbeKey 'id_ed25519' @('-t', 'ed25519', '-N', $empty)
    $rsa = New-ProbeKey 'id_rsa' @('-t', 'rsa', '-b', '4096', '-m', 'PEM', '-N', $empty)
    $encrypted = New-ProbeKey 'id_ed25519_enc' @('-t', 'ed25519', '-N', $KeyPassphrase)

    $publicKeys = @("$ed25519.pub", "$rsa.pub", "$encrypted.pub") | ForEach-Object {
        (Get-Content -LiteralPath $_ -Raw).Trim()
    }
    [IO.File]::WriteAllText((Join-Path $fixtureDir 'authorized_keys'), (($publicKeys -join "`n") + "`n"))

    docker compose up -d --build
    if ($LASTEXITCODE -ne 0) { throw 'docker compose up failed' }

    Write-Host ''
    Write-Host 'SSH probe fixture is running. Paste these into the current shell:'
    Write-Host '  $env:ORBYTE_SSH_PROBE="1"'
    Write-Host '  $env:ORBYTE_SSH_HOST="127.0.0.1"'
    Write-Host '  $env:ORBYTE_SSH_PORT="2222"'
    Write-Host '  $env:ORBYTE_SSH_WEAK_PORT="2223"'
    Write-Host '  $env:ORBYTE_SSH_KBD_PORT="2224"'
    Write-Host '  $env:ORBYTE_SSH_USER="orbyte"'
    Write-Host "  `$env:ORBYTE_SSH_PASSWORD='$Password'"
    Write-Host "  `$env:ORBYTE_SSH_ED25519_KEY='$ed25519'"
    Write-Host "  `$env:ORBYTE_SSH_RSA_KEY='$rsa'"
    Write-Host "  `$env:ORBYTE_SSH_ENCRYPTED_KEY='$encrypted'"
    Write-Host "  `$env:ORBYTE_SSH_KEY_PASSPHRASE='$KeyPassphrase'"
    Write-Host ''
    Write-Host 'Then run:'
    Write-Host '  cargo test -p orbyte-ssh --features probe --test protocol_probe -- --ignored --test-threads=1 --nocapture'
    Write-Host ''
    Write-Host 'Stop with: pwsh scripts/ssh-probe/setup.ps1 -Down'
} finally {
    Pop-Location
}
