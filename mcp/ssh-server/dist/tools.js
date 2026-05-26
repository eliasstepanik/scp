/**
 * MCP tool definitions for the SSH server.
 * Exposes two tools: exec and sudo-exec.
 */
import { z } from 'zod';
import { McpError, ErrorCode } from '@modelcontextprotocol/sdk/types.js';
import { sanitizeError } from './sanitize.js';
// Hard deadline for the entire tool call (connect + exec).
// Must be shorter than the MCP SDK's internal pool/request timeout
// to guarantee we return a clean error rather than letting the SDK timeout fire.
const OUTER_TIMEOUT_MS = 90_000;
function withDeadline(ms, fn) {
    return Promise.race([
        fn(),
        new Promise((_, reject) => setTimeout(() => reject(new Error(`Tool call exceeded ${ms}ms deadline`)), ms)),
    ]);
}
/** Format the result of a command execution for MCP response */
function formatResult(stdout, stderr, exitCode) {
    const parts = [];
    if (stdout.trim()) {
        parts.push(stdout);
    }
    if (stderr.trim()) {
        parts.push(`[stderr]\n${stderr.trim()}`);
    }
    if (exitCode !== 0) {
        parts.push(`[exit code: ${exitCode}]`);
    }
    return parts.join('\n') || '(no output)';
}
/** Register the exec tool on an MCP server */
export function registerExecTool(server, conn) {
    const inputSchema = z.object({
        command: z.string().describe('Shell command to execute on the remote SSH server'),
        description: z.string().optional().describe('Optional description of what this command will do'),
    });
    server.registerTool('exec', {
        description: 'Execute a shell command on the remote SSH server and return the output.',
        inputSchema,
    }, async (args) => {
        const { command } = args;
        if (!command || command.trim().length === 0) {
            throw new McpError(ErrorCode.InvalidParams, 'command must not be empty');
        }
        try {
            const result = await withDeadline(OUTER_TIMEOUT_MS, () => conn.exec(command));
            return {
                content: [
                    {
                        type: 'text',
                        text: formatResult(result.stdout, result.stderr, result.exitCode),
                    },
                ],
            };
        }
        catch (err) {
            throw new McpError(ErrorCode.InternalError, `SSH exec failed: ${sanitizeError(err)}`);
        }
    });
}
/** Register the sudo-exec tool on an MCP server */
export function registerSudoExecTool(server, conn) {
    const inputSchema = z.object({
        command: z.string().describe('Shell command to execute with sudo on the remote SSH server'),
        description: z.string().optional().describe('Optional description of what this command will do'),
    });
    server.registerTool('sudo-exec', {
        description: 'Execute a shell command on the remote SSH server using sudo. Will use sudo password if provided, otherwise assumes passwordless sudo.',
        inputSchema,
    }, async (args) => {
        const { command } = args;
        if (!command || command.trim().length === 0) {
            throw new McpError(ErrorCode.InvalidParams, 'command must not be empty');
        }
        try {
            const result = await withDeadline(OUTER_TIMEOUT_MS, () => conn.sudoExec(command));
            return {
                content: [
                    {
                        type: 'text',
                        text: formatResult(result.stdout, result.stderr, result.exitCode),
                    },
                ],
            };
        }
        catch (err) {
            throw new McpError(ErrorCode.InternalError, `SSH sudo-exec failed: ${sanitizeError(err)}`);
        }
    });
}
//# sourceMappingURL=tools.js.map