/**
 * SSH MCP Server — Entry Point
 *
 * Parses CLI arguments, creates the SSH connection manager,
 * registers MCP tools, and starts the stdio transport.
 *
 * Usage:
 *   node dist/index.js -- \
 *     --host=192.168.178.166 \
 *     --port=22 \
 *     --user=saile2204 \
 *     --password=<password> \
 *     --sudoPassword=<password> \
 *     --timeout=120000 \
 *     --maxChars=none
 */
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { registerSensitive } from './sanitize.js';
import { SSHConnectionManager } from './connection.js';
import { registerExecTool, registerSudoExecTool } from './tools.js';
// ─── CLI Argument Parsing ────────────────────────────────────────────────────
function parseArgs() {
    const args = process.argv.slice(2);
    // Skip leading '--' separator (used in opencode.json command arrays)
    const startIdx = args[0] === '--' ? 1 : 0;
    const argList = args.slice(startIdx);
    const parsed = {};
    for (const arg of argList) {
        const match = arg.match(/^--([^=]+)=(.*)$/);
        if (match) {
            parsed[match[1]] = match[2];
        }
    }
    // Validate required args
    if (!parsed['host'])
        throw new Error('--host is required');
    if (!parsed['user'])
        throw new Error('--user is required');
    if (!parsed['password'])
        throw new Error('--password is required');
    const timeout = parsed['timeout'] ? parseInt(parsed['timeout'], 10) : 120_000;
    const maxCharsRaw = parsed['maxChars'];
    const maxChars = !maxCharsRaw || maxCharsRaw.toLowerCase() === 'none'
        ? null
        : parseInt(maxCharsRaw, 10);
    return {
        host: parsed['host'],
        port: parsed['port'] ? parseInt(parsed['port'], 10) : 22,
        user: parsed['user'],
        password: parsed['password'],
        sudoPassword: parsed['sudoPassword'],
        timeout: isNaN(timeout) ? 120_000 : timeout,
        maxChars: maxChars !== null && isNaN(maxChars) ? null : maxChars,
    };
}
// ─── Main ────────────────────────────────────────────────────────────────────
async function main() {
    let config;
    try {
        config = parseArgs();
    }
    catch (err) {
        process.stderr.write(`[ssh-mcp-server] Argument error: ${err instanceof Error ? err.message : String(err)}\n`);
        process.exit(1);
    }
    // Register sensitive values so they never appear in logs
    registerSensitive(config.password);
    if (config.sudoPassword)
        registerSensitive(config.sudoPassword);
    // Create connection manager (lazy connect — first command triggers connect)
    const conn = new SSHConnectionManager(config);
    // Pre-connect eagerly so the server is ready immediately
    try {
        await conn.connect();
    }
    catch (err) {
        // Log but don't exit — the server will retry on first tool call
        process.stderr.write(JSON.stringify({
            ts: new Date().toISOString(),
            level: 'warn',
            event: 'initial_connect_failed',
            host: config.host,
            error: err instanceof Error ? err.message : String(err),
        }) + '\n');
    }
    // Create MCP server
    const server = new McpServer({
        name: 'ssh-mcp-server',
        version: '1.0.0',
    });
    // Register tools
    registerExecTool(server, conn);
    registerSudoExecTool(server, conn);
    // Graceful shutdown
    const shutdown = async (signal) => {
        process.stderr.write(JSON.stringify({
            ts: new Date().toISOString(),
            level: 'info',
            event: 'shutdown',
            signal,
        }) + '\n');
        await conn.close();
        process.exit(0);
    };
    process.on('SIGTERM', () => void shutdown('SIGTERM'));
    process.on('SIGINT', () => void shutdown('SIGINT'));
    // Start stdio transport
    const transport = new StdioServerTransport();
    await server.connect(transport);
}
main().catch((err) => {
    process.stderr.write(`[ssh-mcp-server] Fatal error: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
});
//# sourceMappingURL=index.js.map