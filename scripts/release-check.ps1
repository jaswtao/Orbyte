# Author: jaswtao · Date: 2026-10-14
param([string]$Gitleaks = 'gitleaks')
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $checks = @(
        @{ Command = 'python'; Arguments = @('scripts/check-release.py') },
        @{ Command = $Gitleaks; Arguments = @('dir', '.', '--redact', '--no-banner') },
        @{ Command = 'python'; Arguments = @('scripts/audit-dependencies.py') },
        @{ Command = 'pnpm'; Arguments = @('audit', '--prod') },
        @{ Command = 'cargo'; Arguments = @('test', '--workspace', '--tests', '--locked') },
        @{ Command = 'pnpm'; Arguments = @('--filter', 'orbyte-desktop', 'test') },
        @{ Command = 'pnpm'; Arguments = @('--filter', 'orbyte-desktop', 'build') },
        @{ Command = 'python'; Arguments = @('scripts/generate-licenses.py', '--check') }
    )
    foreach ($check in $checks) {
        $checkArguments = $check.Arguments
        & $check.Command @checkArguments
        if ($LASTEXITCODE -ne 0) { throw "Release check failed: $($check.Command) $($checkArguments -join ' ')" }
    }
} finally { Pop-Location }
