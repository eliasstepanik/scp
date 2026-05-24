$WorkspaceDir = "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\scp"
$HubBin = "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\scp\target\debug\scp-hub.exe"
$TestConfig = "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\scp\harness\scp-test-resolved.toml"

Write-Host "HubBin: $HubBin"
Write-Host "TestConfig: $TestConfig"
Write-Host "HubBin exists: $(Test-Path $HubBin)"
Write-Host "TestConfig exists: $(Test-Path $TestConfig)"

Write-Host "Starting hub..."
$proc = Start-Process $HubBin -ArgumentList "--config", $TestConfig, "--log-level", "debug" -PassThru -NoNewWindow -RedirectStandardOutput "hub_stdout.txt" -RedirectStandardError "hub_stderr.txt"

Write-Host "Waiting 5 seconds..."
Start-Sleep -Seconds 5

Write-Host "=== STDERR ==="
Get-Content "hub_stderr.txt" -ErrorAction SilentlyContinue

Write-Host "=== STDOUT ==="
Get-Content "hub_stdout.txt" -ErrorAction SilentlyContinue

Write-Host "Killing process..."
if ($proc -and -not $proc.HasExited) {
    $proc.Kill()
}
