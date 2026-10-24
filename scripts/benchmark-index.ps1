# Author: jaswtao · Date: 2026-10-24
param([string]$ManifestPath = 'Cargo.toml', [int[]]$Counts = @(10000, 90000), [string]$BaselineRef = '')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$benchmarkTarget = Join-Path $projectRoot 'target'
if ($BaselineRef) {
    $ManifestPath = & python (Join-Path $PSScriptRoot 'prepare-index-baseline.py') $BaselineRef
    if ($LASTEXITCODE -ne 0) { throw 'Baseline preparation failed' }
}
& cargo build --offline --release --example index_benchmark -p orbyte-files --manifest-path $ManifestPath --target-dir $benchmarkTarget
if ($LASTEXITCODE -ne 0) { throw 'Benchmark build failed' }
foreach ($count in $Counts) {
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = Join-Path $benchmarkTarget 'release/examples/index_benchmark.exe'
    $startInfo.ArgumentList.Add([string]$count)
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $benchmarkProcess = [System.Diagnostics.Process]::Start($startInfo)
    $output = $benchmarkProcess.StandardOutput.ReadToEndAsync()
    $peakBytes = 0L
    $cpuMs = 0.0
    while (-not $benchmarkProcess.WaitForExit(100)) {
        $benchmarkProcess.Refresh()
        if (-not $benchmarkProcess.HasExited) {
            $peakBytes = [Math]::Max($peakBytes, $benchmarkProcess.PeakWorkingSet64)
            $cpuMs = $benchmarkProcess.TotalProcessorTime.TotalMilliseconds
        }
    }
    $resultCode = $benchmarkProcess.ExitCode
    $output.GetAwaiter().GetResult()
    "sampled_peak_working_set_mib=$([Math]::Round($peakBytes / 1MB, 2)) sampled_cpu_ms=$cpuMs"
    $benchmarkProcess.Dispose()
    if ($resultCode -ne 0) { throw "Benchmark failed for $count files" }
}
