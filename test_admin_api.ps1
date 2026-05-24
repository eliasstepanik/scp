cd "C:\Users\Elias Stepanik\OpenCloud\Persönlich\Dev\Projecte\scp"

Write-Host "Starting hub..."
$proc = Start-Process -FilePath ".\target\debug\scp-hub.exe" -ArgumentList "--config", "harness\scp-test-resolved.toml", "--log-level", "warn" -PassThru -NoNewWindow -RedirectStandardOutput "hub_test_stdout.log" -RedirectStandardError "hub_test_stderr.log"

Write-Host "Hub PID: $($proc.Id)"
Write-Host "Waiting 5 seconds..."
Start-Sleep -Seconds 5

Write-Host "Testing health endpoint..."
try {
    $response = Invoke-WebRequest -Uri "http://127.0.0.1:3101/health" -Method GET
    Write-Host "Response status: $($response.StatusCode)"
    Write-Host "Response content type: $($response.Headers['Content-Type'])"
    Write-Host "Response content: $($response.Content)"
    $json = $response.Content | ConvertFrom-Json
    Write-Host "Parsed JSON: $($json | ConvertTo-Json)"
} catch {
    Write-Host "ERROR: $($_.Exception.Message)"
    Write-Host "Exception type: $($_.Exception.GetType().FullName)"
}

Write-Host "Killing hub..."
if ($proc -and -not $proc.HasExited) {
    $proc.Kill()
    $proc.WaitForExit()
}

Write-Host "`n=== STDOUT ===" 
Get-Content "hub_test_stdout.log" -ErrorAction SilentlyContinue | Select-Object -Last 20

Write-Host "`n=== STDERR ===" 
Get-Content "hub_test_stderr.log" -ErrorAction SilentlyContinue
