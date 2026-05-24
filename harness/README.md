# SCP Test Harness

Comprehensive test suite for the SCP Hub and MCP integration.

## Prerequisites

- PowerShell 7.0 or later
- Rust toolchain (for building)
- `mcp-remote` installed globally (optional, for opencode registration)

## Running Tests

```powershell
cd harness
pwsh .\Run-Tests.ps1
```

### Options

- `-SkipBuild`: Skip the cargo build step (use if binaries are already built)
- `-RegisterOpencode`: Register the SCP hub in opencode configuration
- `-MaxRuns N`: Maximum number of test runs before giving up (default: 10)
- `-HubPort N`: Hub port (default: 3100)
- `-AdminPort N`: Admin API port (default: 3101)

### Examples

```powershell
# Run with skip build
pwsh .\Run-Tests.ps1 -SkipBuild

# Run with opencode registration
pwsh .\Run-Tests.ps1 -RegisterOpencode

# Run with custom max runs
pwsh .\Run-Tests.ps1 -MaxRuns 5
```

## Test Coverage

The test harness covers 8 test groups with a total of 47 tests:

### 1. Admin API (6 tests)
- Health endpoint status field
- Health endpoint server count
- Prometheus metrics format
- JSON metrics endpoint
- Sessions list endpoint
- Tools list endpoint

### 2. Session Management (9 tests)
- Session creation via initialize
- Session reuse
- Invalid session handling
- Session deletion
- Session tracking in admin API
- Authentication enforcement
- Token validation

### 3. Tools List (5 tests)
- Tools array presence
- SCP extension tools (scp_get_more, scp_info, scp_budget, scp_budget_reset)
- Mock backend tool availability
- Tool count limits
- Rate limit headers

### 4. Tools Call (7 tests)
- Echo tool invocation
- Nonexistent tool error handling
- Request ID preservation
- SCP info tool
- SCP budget tool
- SCP budget reset tool
- SCP get more tool

### 5. Filter Pipeline (2 tests)
- Budget tracking across requests
- Message content preservation

### 6. Rate Limiting (3 tests)
- Rate limit header presence
- Rate limit enforcement (429 response)
- Retry-After header on rate limit

### 7. Graceful Shutdown (2 tests)
- Hub process termination
- Health endpoint unreachability after shutdown

### 8. (Reserved for future tests)

## Not Tested

The following features are not covered by this harness:

- Circuit breaker functionality
- Server-Sent Events (SSE) streaming
- Embedding-based tool scoring
- Request cancellation (notifications/cancelled)

## Configuration

The test harness uses `scp-test.toml` as a template. At runtime, it:

1. Reads the template
2. Replaces `${MOCK_SERVER_BIN}` with the actual path to the mock server binary
3. Writes the resolved config to `scp-test-resolved.toml`
4. Passes the resolved config to the hub

## Troubleshooting

### Hub fails to start
- Ensure binaries are built: `cargo build --workspace`
- Check that ports 3100 and 3101 are available
- Review hub logs for errors

### Tests fail intermittently
- Increase `-MaxRuns` to allow more retries
- Check system load and available resources
- Verify mock server is functioning correctly

### Rate limit tests fail
- Ensure the ratelimited profile is configured correctly
- Check that rate limit enforcement is enabled in the hub
- Verify timing between requests (should be < 1 second)

## Architecture

The harness consists of:

- **scp-test.toml**: Configuration template with placeholders
- **Run-Tests.ps1**: Main test script with helper functions and test groups
- **scp-test-resolved.toml**: Generated at runtime with actual binary paths

The script:
1. Builds the project (unless `-SkipBuild`)
2. Generates the resolved config
3. Starts the hub process
4. Runs all test groups in sequence
5. Retries failed tests up to `-MaxRuns` times
6. Performs graceful shutdown
7. Cleans up temporary files
