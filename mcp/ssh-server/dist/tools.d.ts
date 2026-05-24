/**
 * MCP tool definitions for the SSH server.
 * Exposes two tools: exec and sudo-exec.
 */
import type { SSHConnectionManager } from './connection.js';
/** Register the exec tool on an MCP server */
export declare function registerExecTool(server: import('@modelcontextprotocol/sdk/server/mcp.js').McpServer, conn: SSHConnectionManager): void;
/** Register the sudo-exec tool on an MCP server */
export declare function registerSudoExecTool(server: import('@modelcontextprotocol/sdk/server/mcp.js').McpServer, conn: SSHConnectionManager): void;
//# sourceMappingURL=tools.d.ts.map