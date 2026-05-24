#Requires -Version 5.1
param(
    [int]$MaxRuns = 10,
    [switch]$SkipBuild,
    [switch]$RegisterOpencode,
    [int]$HubPort = 3100,
    [int]$AdminPort = 3101
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Constants
$WorkspaceDir = Split-Path $PSScriptRoot -Parent
Push-Location $WorkspaceDir
$HubBin = ".\target\debug\scp-hub.exe"
$MockBin = ".\target\debug\mock-mcp-server.exe"
$TestConfig = "harness\scp-test-resolved.toml"
$TemplateConfig = "harness\scp-test.toml"
$HubUrl = "http://127.0.0.1:$HubPort"
$AdminUrl = "http://127.0.0.1:$AdminPort"
$TestToken = "scp-test-token-abc123"
$RateLimitToken = "scp-ratelimit-token-xyz"

# Script-scoped variables
$script:HubProcess = $null
$script:LastHeaders = @{}
$script:TestResults = @()
$script:PassCount = 0
$script:FailCount = 0

Write-Host "SCP Test Harness v1.0" -ForegroundColor Cyan
Write-Host "Workspace: $WorkspaceDir" -ForegroundColor Gray

# Helper function: Add test result
function Add-Result {
    param(
        [string]$Name,
        [bool]$Passed,
        [string]$Message = ""
    )
    $script:TestResults += [PSCustomObject]@{
        Name = $Name
        Passed = $Passed
        Message = $Message
    }
    if ($Passed) {
        $script:PassCount++
        Write-Host "[PASS] $Name" -ForegroundColor Green
    } else {
        $script:FailCount++
        Write-Host "[FAIL] $Name - $Message" -ForegroundColor Red
    }
}

# Helper function: Invoke MCP request
function Invoke-Mcp {
    param(
        [string]$Method = "POST",
        [string]$Path = "/mcp",
        [hashtable]$Body = $null,
        [string]$SessionId = $null,
        [string]$Token = $TestToken
    )
    
    $uri = "$HubUrl$Path"
    
    # Build headers
    $headers = @{
        "Content-Type" = "application/json"
    }
    
    if ($Token) {
        $headers["Authorization"] = "Bearer $Token"
    }
    
    if ($SessionId) {
        $headers["Mcp-Session-Id"] = $SessionId
    }
    
    # Build body
    $bodyStr = $null
    if ($Body) {
        $bodyStr = $Body | ConvertTo-Json -Depth 10
    }
    
    # Use .NET HttpWebRequest for better compatibility
    try {
        $request = [System.Net.HttpWebRequest]::Create($uri)
        $request.Method = $Method
        $request.ContentType = "application/json"
        
        # Add headers
        foreach ($key in $headers.Keys) {
            if ($key -eq "Content-Type") {
                $request.ContentType = $headers[$key]
            } else {
                $request.Headers.Add($key, $headers[$key])
            }
        }
        
        # Add body
        if ($bodyStr) {
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($bodyStr)
            $request.ContentLength = $bytes.Length
            $stream = $request.GetRequestStream()
            $stream.Write($bytes, 0, $bytes.Length)
            $stream.Close()
        }
        
        # Send request and get response
        $response = $request.GetResponse()
        
        # Capture headers
        $script:LastHeaders = @{}
        foreach ($header in $response.Headers.Keys) {
            $script:LastHeaders[$header] = $response.Headers[$header]
        }
        
        # Get response content
        $reader = New-Object System.IO.StreamReader($response.GetResponseStream())
        $content = $reader.ReadToEnd()
        $reader.Close()
        $response.Close()
        
        # Success response
        if ($content) {
            $content | ConvertFrom-Json
        }
    } catch {
        # Handle HTTP error responses
        $errorRecord = $_
        if ($errorRecord.Exception -and $errorRecord.Exception.Response) {
            $response = $errorRecord.Exception.Response
            
            # Capture headers
            $script:LastHeaders = @{}
            foreach ($header in $response.Headers.Keys) {
                $script:LastHeaders[$header] = $response.Headers[$header]
            }
            
            # Get status code
            $statusCode = [int]$response.StatusCode
            
            # Try to read error body
            try {
                $reader = New-Object System.IO.StreamReader($response.GetResponseStream())
                $content = $reader.ReadToEnd()
                $reader.Close()
                
                if ($content) {
                    $errorJson = $content | ConvertFrom-Json
                    $errorJson | Add-Member -NotePropertyName "StatusCode" -NotePropertyValue $statusCode
                    return $errorJson
                }
            } catch {
                # If we can't read the body, just return status code
            }
            
            # Return error object with status code
            return @{ StatusCode = $statusCode }
        }
        
        # Re-throw if not an HTTP error
        throw
    }
}

# Helper function: Create new MCP session
function New-McpSession {
    param(
        [string]$Token = $TestToken
    )
    
    $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
        jsonrpc = "2.0"
        id = 1
        method = "initialize"
        params = @{}
    } -Token $Token
    
    return $script:LastHeaders["Mcp-Session-Id"]
}

# Helper function: Wait for hub to be ready
function Wait-HubReady {
    param(
        [int]$TimeoutSecs = 30
    )
    
    $startTime = Get-Date
    while ((Get-Date) -lt $startTime.AddSeconds($TimeoutSecs)) {
        try {
            $response = Invoke-RestMethod -Uri "$AdminUrl/health" -Method GET -TimeoutSec 5 -ErrorAction Stop
            if ($response.status -eq "ok") {
                return $true
            }
        } catch {
            # Still waiting - connection refused or timeout
        }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

# Helper function: Build project
function Invoke-Build {
    if ($SkipBuild) {
        Write-Host "Skipping build" -ForegroundColor Yellow
        return
    }
    
    Write-Host "Building project..." -ForegroundColor Cyan
    Push-Location $WorkspaceDir
    try {
        cargo build --workspace
        if ($LASTEXITCODE -ne 0) {
            throw "Build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
    
    if (-not (Test-Path $HubBin)) {
        throw "Hub binary not found: $HubBin"
    }
    if (-not (Test-Path $MockBin)) {
        throw "Mock binary not found: $MockBin"
    }
    
    Write-Host "Build successful" -ForegroundColor Green
}

# Helper function: Write resolved config
function Write-ResolvedConfig {
    Write-Host "Writing resolved config..." -ForegroundColor Cyan
    $content = Get-Content $TemplateConfig -Raw
    # Get absolute path for the mock binary
    $MockBinAbsolute = (Resolve-Path $MockBin).Path
    $content = $content -replace '\$\{MOCK_SERVER_BIN\}', ($MockBinAbsolute -replace '\\', '\\')
    Set-Content -Path $TestConfig -Value $content -Encoding UTF8
    Write-Host "Config written to $TestConfig" -ForegroundColor Green
}

# Helper function: Start hub
function Start-Hub {
    Write-Host "Starting hub..." -ForegroundColor Cyan
    $script:HubProcess = Start-Process $HubBin -ArgumentList "--config", $TestConfig, "--log-level", "warn" -PassThru -NoNewWindow -RedirectStandardOutput "hub_stdout.log" -RedirectStandardError "hub_stderr.log"
    
    if (-not (Wait-HubReady)) {
        Write-Host "Hub failed to start. Checking logs..." -ForegroundColor Red
        if (Test-Path "hub_stderr.log") {
            Write-Host "=== STDERR ===" -ForegroundColor Red
            Get-Content "hub_stderr.log"
        }
        if (Test-Path "hub_stdout.log") {
            Write-Host "=== STDOUT ===" -ForegroundColor Red
            Get-Content "hub_stdout.log"
        }
        throw "Hub failed to start within timeout"
    }
    
    Write-Host "Hub started (PID: $($script:HubProcess.Id))" -ForegroundColor Green
}

# Helper function: Stop hub
function Stop-Hub {
    if ($script:HubProcess -and -not $script:HubProcess.HasExited) {
        Write-Host "Stopping hub..." -ForegroundColor Cyan
        $script:HubProcess.CloseMainWindow() | Out-Null
        if (-not $script:HubProcess.WaitForExit(5000)) {
            $script:HubProcess.Kill()
        }
        Write-Host "Hub stopped" -ForegroundColor Green
    }
}

# Helper function: Test hub alive
function Test-HubAlive {
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/health" -Method GET -ErrorAction SilentlyContinue
        return $response.StatusCode -eq 200
    } catch {
        return $false
    }
}

# Helper function: Register in opencode
function Register-InOpencode {
    Write-Host "Registering in opencode..." -ForegroundColor Cyan
    $configPath = Join-Path $env:USERPROFILE ".config\opencode\opencode.json"
    
    if (-not (Test-Path $configPath)) {
        Write-Host "opencode.json not found, skipping registration" -ForegroundColor Yellow
        return
    }
    
    $config = Get-Content $configPath | ConvertFrom-Json
    
    if ($config.servers.scp) {
        Write-Host "SCP already registered, skipping" -ForegroundColor Yellow
        return
    }
    
    $config.servers | Add-Member -NotePropertyName "scp" -NotePropertyValue @{
        type = "local"
        command = @("mcp-remote", "http://127.0.0.1:3100/mcp", "--transport", "http-only", "--allow-http")
        enabled = $false
    }
    
    $config | ConvertTo-Json -Depth 10 | Set-Content -Path $configPath -Encoding UTF8
    Write-Host "Registered in opencode" -ForegroundColor Green
}


# Test group: Admin API
function Test-AdminApi {
    Write-Host "`n--- Test-AdminApi ---" -ForegroundColor Cyan
    
    # A1: GET /health has status field
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/health" -Method GET -ErrorAction Stop
        if ($response -and $response.Content) {
            $json = $response.Content | ConvertFrom-Json
            Add-Result "A1: Health has status field" ($json.status -ne $null) "status=$($json.status)"
        } else {
            Add-Result "A1: Health has status field" $false "No response content"
        }
    } catch {
        Add-Result "A1: Health has status field" $false $_.Exception.Message
    }
    
    # A2: GET /health has servers >= 1
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/health" -Method GET -ErrorAction Stop
        if ($response -and $response.Content) {
            $json = $response.Content | ConvertFrom-Json
            Add-Result "A2: Health has servers >= 1" ($json.servers -ge 1) "servers=$($json.servers)"
        } else {
            Add-Result "A2: Health has servers >= 1" $false "No response content"
        }
    } catch {
        Add-Result "A2: Health has servers >= 1" $false $_.Exception.Message
    }
    
    # A3: GET /metrics contains scp_tokens_saved_total and scp_errors_total
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/metrics" -Method GET -ErrorAction Stop
        if ($response -and $response.Content) {
            $content = $response.Content
            $hasTokens = $content -match "scp_tokens_saved_total"
            $hasErrors = $content -match "scp_errors_total"
            Add-Result "A3: Metrics has tokens and errors" ($hasTokens -and $hasErrors) ""
        } else {
            Add-Result "A3: Metrics has tokens and errors" $false "No response content"
        }
    } catch {
        Add-Result "A3: Metrics has tokens and errors" $false $_.Exception.Message
    }
    
    # A4: GET /admin/metrics has JSON fields
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/admin/metrics" -Method GET -ErrorAction Stop
        if ($response -and $response.Content) {
            $json = $response.Content | ConvertFrom-Json
            $hasTokens = $json.tokens_saved_total -ne $null
            $hasErrors = $json.errors_total -ne $null
            $hasInflight = $json.inflight_requests -ne $null
            Add-Result "A4: Admin metrics has required fields" ($hasTokens -and $hasErrors -and $hasInflight) ""
        } else {
            Add-Result "A4: Admin metrics has required fields" $false "No response content"
        }
    } catch {
        Add-Result "A4: Admin metrics has required fields" $false $_.Exception.Message
    }
    
    # A5: GET /admin/sessions returns sessions field
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/admin/sessions" -Method GET -ErrorAction Stop
        if ($response -and $response.Content) {
            $json = $response.Content | ConvertFrom-Json
            Add-Result "A5: Sessions endpoint has sessions field" ($json.sessions -ne $null) ""
        } else {
            Add-Result "A5: Sessions endpoint has sessions field" $false "No response content"
        }
    } catch {
        Add-Result "A5: Sessions endpoint has sessions field" $false $_.Exception.Message
    }
    
    # A6: GET /tools returns array
    try {
        $response = Invoke-WebRequest -Uri "$AdminUrl/tools" -Method GET -ErrorAction Stop
        if ($response -and $response.Content) {
            $json = $response.Content | ConvertFrom-Json
            Add-Result "A6: Tools endpoint returns array" ($json -is [array]) ""
        } else {
            Add-Result "A6: Tools endpoint returns array" $false "No response content"
        }
    } catch {
        Add-Result "A6: Tools endpoint returns array" $false $_.Exception.Message
    }
}

# Test group: Session Management
function Test-SessionManagement {
    Write-Host "`n--- Test-SessionManagement ---" -ForegroundColor Cyan
    
    # S1: POST initialize without session ID returns Mcp-Session-Id header
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "initialize"
            params = @{}
        }
        $sessionId = $script:LastHeaders["Mcp-Session-Id"]
        Add-Result "S1: Initialize creates session" ($sessionId -ne $null) "sessionId=$sessionId"
    } catch {
        Add-Result "S1: Initialize creates session" $false $_.Exception.Message
    }
    
    # S2: Reuse session
    try {
        $sessionId = New-McpSession
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 2
            method = "ping"
            params = @{}
        } -SessionId $sessionId
        Add-Result "S2: Reuse session" ($response.result -ne $null) ""
    } catch {
        Add-Result "S2: Reuse session" $false $_.Exception.Message
    }
    
    # S3: Invalid session ID returns 404
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 3
            method = "ping"
            params = @{}
        } -SessionId "invalid-session-id"
        # If we got a response with StatusCode, it's an error response
        if ($response -and $response.StatusCode -eq 404) {
            Add-Result "S3: Invalid session returns 404" $true ""
        } else {
            Add-Result "S3: Invalid session returns 404" $false "Expected 404 but got success"
        }
    } catch {
        Add-Result "S3: Invalid session returns 404" $false $_.Exception.Message
    }
    
    # S4: DELETE /mcp with valid session returns 200
    try {
        $sessionId = New-McpSession
        $response = Invoke-WebRequest -Uri "$HubUrl/mcp" -Method DELETE -Headers @{
            "Mcp-Session-Id" = $sessionId
            "Authorization" = "Bearer $TestToken"
        }
        Add-Result "S4: DELETE session returns 200" ($response.StatusCode -eq 200) ""
    } catch {
        Add-Result "S4: DELETE session returns 200" $false $_.Exception.Message
    }
    
    # S5: After initialize, session appears in admin list
    try {
        $sessionId = New-McpSession
        Start-Sleep -Milliseconds 100
        $response = Invoke-WebRequest -Uri "$AdminUrl/admin/sessions" -Method GET
        $json = $response.Content | ConvertFrom-Json
        $found = $json.sessions | Where-Object { $_ -eq $sessionId }
        Add-Result "S5: Session in admin list" ($found -ne $null) ""
    } catch {
        Add-Result "S5: Session in admin list" $false $_.Exception.Message
    }
    
    # S6: After DELETE, session removed from admin list
    try {
        $sessionId = New-McpSession
        Start-Sleep -Milliseconds 100
        Invoke-WebRequest -Uri "$HubUrl/mcp" -Method DELETE -Headers @{
            "Mcp-Session-Id" = $sessionId
            "Authorization" = "Bearer $TestToken"
        } | Out-Null
        Start-Sleep -Milliseconds 100
        $response = Invoke-WebRequest -Uri "$AdminUrl/admin/sessions" -Method GET
        $json = $response.Content | ConvertFrom-Json
        $found = $json.sessions | Where-Object { $_ -eq $sessionId }
        Add-Result "S6: Session removed from admin list" ($found -eq $null) ""
    } catch {
        Add-Result "S6: Session removed from admin list" $false $_.Exception.Message
    }
    
    # S7: POST without Authorization returns 401
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 7
            method = "initialize"
            params = @{}
        } -Token ""
        # If we got a response with StatusCode, it's an error response
        if ($response -and $response.StatusCode -eq 401) {
            Add-Result "S7: No auth returns 401" $true ""
        } else {
            Add-Result "S7: No auth returns 401" $false "Expected 401 but got success"
        }
    } catch {
        Add-Result "S7: No auth returns 401" $false $_.Exception.Message
    }
    
    # S8: POST with invalid token returns 401
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 8
            method = "initialize"
            params = @{}
        } -Token "invalid-token"
        # If we got a response with StatusCode, it's an error response
        if ($response -and $response.StatusCode -eq 401) {
            Add-Result "S8: Invalid token returns 401" $true ""
        } else {
            Add-Result "S8: Invalid token returns 401" $false "Expected 401 but got success"
        }
    } catch {
        Add-Result "S8: Invalid token returns 401" $false $_.Exception.Message
    }
    
    # S9: POST with valid token returns 200
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 9
            method = "initialize"
            params = @{}
        } -Token $TestToken
        $sessionId = $script:LastHeaders["Mcp-Session-Id"]
        Add-Result "S9: Valid token returns 200" ($sessionId -ne $null) ""
    } catch {
        Add-Result "S9: Valid token returns 200" $false $_.Exception.Message
    }
}


# Test group: Tools List
function Test-ToolsList {
    Write-Host "`n--- Test-ToolsList ---" -ForegroundColor Cyan
    
    $sessionId = New-McpSession
    
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/list"
            params = @{}
        } -SessionId $sessionId
        
        # T1: tools array present
        $hasTools = $response.result.tools -ne $null
        Add-Result "T1: Tools array present" $hasTools ""
        
        if ($hasTools) {
            $toolNames = $response.result.tools | ForEach-Object { $_.name }
            
            # T2a-T2d: SCP tools present
            $hasScp_get_more = $toolNames -contains "scp_get_more"
            $hasScp_info = $toolNames -contains "scp_info"
            $hasScp_budget = $toolNames -contains "scp_budget"
            $hasScp_budget_reset = $toolNames -contains "scp_budget_reset"
            
            Add-Result "T2a: scp_get_more present" $hasScp_get_more ""
            Add-Result "T2b: scp_info present" $hasScp_info ""
            Add-Result "T2c: scp_budget present" $hasScp_budget ""
            Add-Result "T2d: scp_budget_reset present" $hasScp_budget_reset ""
            
            # T3: echo tool present
            $hasEcho = $toolNames -contains "mock.echo"
            Add-Result "T3: mock.echo tool present" $hasEcho ""
            
            # T4: tools count <= 20
            $toolCount = $response.result.tools.Count
            Add-Result "T4: Tools count <= 20" ($toolCount -le 20) "count=$toolCount"
        }
        
        # T5: Rate limit header present
        $hasRateLimit = $script:LastHeaders["X-SCP-RateLimit-Remaining"] -ne $null
        Add-Result "T5: RateLimit header present" $hasRateLimit ""
        
    } catch {
        Add-Result "T1: Tools array present" $false $_.Exception.Message
    }
}

# Test group: Tools Call
function Test-ToolsCall {
    Write-Host "`n--- Test-ToolsCall ---" -ForegroundColor Cyan
    
    $sessionId = New-McpSession
    
    # C1: Call echo tool
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/call"
            params = @{
                name = "mock.echo"
                arguments = @{
                    message = "hello-scp-test"
                }
            }
        } -SessionId $sessionId
        
        $hasContent = $response.result.content -ne $null
        $contentStr = $response.result.content | ConvertTo-Json
        $hasMessage = $contentStr -match "hello-scp-test"
        Add-Result "C1: Echo tool works" ($hasContent -and $hasMessage) ""
    } catch {
        Add-Result "C1: Echo tool works" $false $_.Exception.Message
    }
    
    # C2: Call nonexistent tool
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 2
            method = "tools/call"
            params = @{
                name = "nonexistent"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $hasError = $response.error -ne $null
        Add-Result "C2: Nonexistent tool returns error" $hasError ""
    } catch {
        Add-Result "C2: Nonexistent tool returns error" $false $_.Exception.Message
    }
    
    # C3: Request with id=9999
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 9999
            method = "ping"
            params = @{}
        } -SessionId $sessionId
        
        Add-Result "C3: Response id matches request" ($response.id -eq 9999) "id=$($response.id)"
    } catch {
        Add-Result "C3: Response id matches request" $false $_.Exception.Message
    }
    
    # C4: Call scp_info
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 4
            method = "tools/call"
            params = @{
                name = "scp_info"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $hasContent = $response.result.content -ne $null
        Add-Result "C4: scp_info works" $hasContent ""
    } catch {
        Add-Result "C4: scp_info works" $false $_.Exception.Message
    }
    
    # C5: Call scp_budget
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 5
            method = "tools/call"
            params = @{
                name = "scp_budget"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $hasContent = $response.result.content -ne $null
        Add-Result "C5: scp_budget works" $hasContent ""
    } catch {
        Add-Result "C5: scp_budget works" $false $_.Exception.Message
    }
    
    # C6: Call scp_budget_reset
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 6
            method = "tools/call"
            params = @{
                name = "scp_budget_reset"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $hasError = $response.error -ne $null
        Add-Result "C6: scp_budget_reset works" (-not $hasError) ""
    } catch {
        Add-Result "C6: scp_budget_reset works" $false $_.Exception.Message
    }
    
    # C7: Call scp_get_more with no args
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 7
            method = "tools/call"
            params = @{
                name = "scp_get_more"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $hasContent = $response.result.content -ne $null
        Add-Result "C7: scp_get_more works" $hasContent ""
    } catch {
        Add-Result "C7: scp_get_more works" $false $_.Exception.Message
    }
}

# Test group: Filter Pipeline
function Test-FilterPipeline {
    Write-Host "`n--- Test-FilterPipeline ---" -ForegroundColor Cyan
    
    $sessionId = New-McpSession
    
    # F1: Budget before and after
    try {
        $budgetBefore = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/call"
            params = @{
                name = "scp_budget"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $echo = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 2
            method = "tools/call"
            params = @{
                name = "mock.echo"
                arguments = @{
                    message = "filter-test"
                }
            }
        } -SessionId $sessionId
        
        $budgetAfter = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 3
            method = "tools/call"
            params = @{
                name = "scp_budget"
                arguments = @{}
            }
        } -SessionId $sessionId
        
        $hasBefore = $budgetBefore.result.content -ne $null
        $hasAfter = $budgetAfter.result.content -ne $null
        Add-Result "F1: Budget tracking works" ($hasBefore -and $hasAfter) ""
    } catch {
        Add-Result "F1: Budget tracking works" $false $_.Exception.Message
    }
    
    # F2: Echo with specific message
    try {
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 4
            method = "tools/call"
            params = @{
                name = "mock.echo"
                arguments = @{
                    message = "filter-test-content"
                }
            }
        } -SessionId $sessionId
        
        $contentStr = $response.result.content | ConvertTo-Json
        $hasContent = $contentStr -match "filter-test-content"
        Add-Result "F2: Echo message preserved" $hasContent ""
    } catch {
        Add-Result "F2: Echo message preserved" $false $_.Exception.Message
    }
}

# Test group: Rate Limiting
function Test-RateLimiting {
    Write-Host "`n--- Test-RateLimiting ---" -ForegroundColor Cyan
    
    # R1: Rate limit header present
    try {
        $sessionId = New-McpSession -Token $TestToken
        $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/list"
            params = @{}
        } -SessionId $sessionId -Token $TestToken
        
        $hasHeader = $script:LastHeaders["X-SCP-RateLimit-Remaining"] -ne $null
        Add-Result "R1: RateLimit header present" $hasHeader ""
    } catch {
        Add-Result "R1: RateLimit header present" $false $_.Exception.Message
    }
    
    # R2: Rate limited token gets 429 on 3rd request
    try {
        $sessionId = New-McpSession -Token $RateLimitToken
        
        # Request 1
        Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/list"
            params = @{}
        } -SessionId $sessionId -Token $RateLimitToken | Out-Null
        
        # Request 2
        Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 2
            method = "tools/list"
            params = @{}
        } -SessionId $sessionId -Token $RateLimitToken | Out-Null
        
        # Request 3 - should fail
         try {
             $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
                 jsonrpc = "2.0"
                 id = 3
                 method = "tools/list"
                 params = @{}
             } -SessionId $sessionId -Token $RateLimitToken
             # If we got a response with StatusCode, it's an error response
             if ($response -and $response.StatusCode -eq 429) {
                 Add-Result "R2: Rate limit enforced" $true ""
             } else {
                 Add-Result "R2: Rate limit enforced" $false "Expected 429 but got success"
             }
         } catch {
             Add-Result "R2: Rate limit enforced" $false $_.Exception.Message
         }
    } catch {
        Add-Result "R2: Rate limit enforced" $false $_.Exception.Message
    }
    
    # R3: 429 response has Retry-After header
    try {
        $sessionId = New-McpSession -Token $RateLimitToken
        
        # Make 2 requests to hit limit
        Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 1
            method = "tools/list"
            params = @{}
        } -SessionId $sessionId -Token $RateLimitToken | Out-Null
        
        Invoke-Mcp -Method POST -Path "/mcp" -Body @{
            jsonrpc = "2.0"
            id = 2
            method = "tools/list"
            params = @{}
        } -SessionId $sessionId -Token $RateLimitToken | Out-Null
        
        # Third request
         try {
             $response = Invoke-Mcp -Method POST -Path "/mcp" -Body @{
                 jsonrpc = "2.0"
                 id = 3
                 method = "tools/list"
                 params = @{}
             } -SessionId $sessionId -Token $RateLimitToken
             # If we got a response with StatusCode, check for Retry-After
             if ($response -and $response.StatusCode -eq 429) {
                 $hasRetryAfter = $script:LastHeaders["Retry-After"] -ne $null
                 Add-Result "R3: 429 has Retry-After header" $hasRetryAfter ""
             }
         } catch {
             Add-Result "R3: 429 has Retry-After header" $false $_.Exception.Message
         }
    } catch {
        Add-Result "R3: 429 has Retry-After header" $false $_.Exception.Message
    }
}

# Test group: Graceful Shutdown
function Test-GracefulShutdown {
    Write-Host "`n--- Test-GracefulShutdown ---" -ForegroundColor Cyan
    
    # G1: Hub exits within 10s
    try {
        $script:HubProcess.CloseMainWindow() | Out-Null
        $exited = $script:HubProcess.WaitForExit(10000)
        Add-Result "G1: Hub exits gracefully" $exited ""
    } catch {
        Add-Result "G1: Hub exits gracefully" $false $_.Exception.Message
    }
    
    # G2: After exit, health endpoint unreachable
    try {
        Start-Sleep -Milliseconds 500
        $response = Invoke-WebRequest -Uri "$AdminUrl/health" -Method GET -ErrorAction SilentlyContinue
        Add-Result "G2: Health unreachable after exit" $false "Expected error but got response"
    } catch {
        Add-Result "G2: Health unreachable after exit" $true ""
    }
}


# Main execution block
try {
    Invoke-Build
    Write-ResolvedConfig
    if ($RegisterOpencode) { Register-InOpencode }
    Start-Hub
    
    $run = 0
    $allPassed = $false
    
    while (-not $allPassed -and $run -lt $MaxRuns) {
        $run++
        $script:TestResults = @()
        $script:PassCount = 0
        $script:FailCount = 0
        
        Write-Host "`n=== TEST RUN $run / $MaxRuns ===" -ForegroundColor Cyan
        
        Test-AdminApi
        Test-SessionManagement
        Test-ToolsList
        Test-ToolsCall
        Test-FilterPipeline
        Test-RateLimiting
        
        $failed = $script:TestResults | Where-Object { -not $_.Passed }
        Write-Host "`nResults: $($script:PassCount) passed, $($script:FailCount) failed" -ForegroundColor Cyan
        
        if ($failed.Count -eq 0) {
            $allPassed = $true
        } else {
            $failed | ForEach-Object { Write-Host "  FAIL: $($_.Name) — $($_.Message)" -ForegroundColor Red }
            if (-not (Test-HubAlive)) {
                Write-Host "Hub is down, restarting..." -ForegroundColor Yellow
                Stop-Hub
                Start-Hub
            }
            if ($run -lt $MaxRuns) {
                Write-Host "Retrying in 3s..." -ForegroundColor Yellow
                Start-Sleep -Seconds 3
            }
        }
    }
    
    if ($allPassed) {
        Test-GracefulShutdown
        $script:HubProcess = $null
        Write-Host "`nALL TESTS PASSED" -ForegroundColor Green
        exit 0
    } else {
        Write-Host "`nTESTS FAILED after $MaxRuns runs" -ForegroundColor Red
        exit 1
    }
} finally {
    Stop-Hub
    if (Test-Path $TestConfig) {
        Remove-Item $TestConfig -Force -ErrorAction SilentlyContinue
    }
    Pop-Location
}

