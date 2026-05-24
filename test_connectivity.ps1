Push-Location "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\scp"

Write-Host "Starting hub..."
$proc = Start-Process -FilePath ".\target\debug\scp-hub.exe" -ArgumentList "--config", "harness\scp-test-resolved.toml", "--log-level", "info" -PassThru -NoNewWindow -RedirectStandardOutput "hub_test_stdout.log" -RedirectStandardError "hub_test_stderr.log"

Write-Host "Hub PID: $($proc.Id)"
Write-Host "Waiting 5 seconds for hub to start..."
Start-Sleep -Seconds 5

Write-Host "Testing health endpoint..."
try {
    $result = Invoke-RestMethod "http://127.0.0.1:3101/health" -TimeoutSec 5
    Write-Host "SUCCESS: Health endpoint responded" -ForegroundColor Green
    Write-Host ($result | ConvertTo-Json)
} catch {
    Write-Host "FAILED: $($_.Exception.Message)" -ForegroundColor Red
}

Write-Host "Killing hub..."
if ($proc -and -not $proc.HasExited) {
    $proc.Kill()
    $proc.WaitForExit()
}

Write-Host "`n=== STDOUT (last 30 lines) ===" -ForegroundColor Cyan
Get-Content "hub_test_stdout.log" -ErrorAction SilentlyContinue | Select-Object -Last 30

Write-Host "`n=== STDERR ===" -ForegroundColor Cyan
Get-Content "hub_test_stderr.log" -ErrorAction SilentlyContinue

Pop-Location
