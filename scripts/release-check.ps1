# Author: jaswtao · Date: 2026-10-14
param([string]$Gitleaks = 'gitleaks', [switch]$SshProbe)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $checks = @(
        @{ Command = 'python'; Arguments = @('scripts/check-release.py') },
        @{ Command = 'python'; Arguments = @('-m', 'unittest', 'discover', '-s', 'scripts/tests') },
        @{ Command = $Gitleaks; Arguments = @('dir', '.', '--redact', '--no-banner') },
        @{ Command = 'python'; Arguments = @('scripts/audit-dependencies.py') },
        @{ Command = 'pnpm'; Arguments = @('audit', '--prod') },
        @{ Command = 'cargo'; Arguments = @('test', '--workspace', '--tests', '--locked') },
        @{ Command = 'pnpm'; Arguments = @('--filter', 'orbyte-desktop', 'test') },
        @{ Command = 'pnpm'; Arguments = @('--filter', 'orbyte-desktop', 'build') },
        @{ Command = 'python'; Arguments = @('scripts/generate-licenses.py', '--check') }
    )
    if ($SshProbe) {
        # Requires the isolated fixture from scripts/ssh-probe and its environment.
        $checks += @{ Command = 'cargo'; Arguments = @('test', '-p', 'orbyte-ssh', '--features', 'probe', '--test', 'protocol_probe', '--locked', '--', '--ignored', '--test-threads=1', '--nocapture') }
    }
    foreach ($check in $checks) {
        $checkArguments = $check.Arguments
        & $check.Command @checkArguments
        if ($LASTEXITCODE -ne 0) { throw "Release check failed: $($check.Command) $($checkArguments -join ' ')" }
    }
} finally { Pop-Location }
