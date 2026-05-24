Push-Location "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\scp"

Write-Host "Starting hub..."
Write-Host "Current dir: $(Get-Location)"

$proc = Start-Process ".\target\debug\scp-hub.exe" -ArgumentList "--config", ".\harness\scp-test-resolved.toml", "--log-level", "debug" -PassThru -NoNewWindow -RedirectStandardOutput "hub_stdout.txt" -RedirectStandardError "hub_stderr.txt"

Write-Host "Hub PID: $($proc.Id)"
Write-Host "Waiting 5 seconds..."
Start-Sleep -Seconds 5

Write-Host "=== STDERR ==="
Get-Content "hub_stderr.txt" -ErrorAction SilentlyContinue

Write-Host "=== STDOUT ==="
Get-Content "hub_stdout.txt" -ErrorAction SilentlyContinue

Write-Host "Killing process..."
if ($proc -and -not $proc.HasExited) {
    $proc.Kill()
    $proc.WaitForExit()
}

Pop-Location
