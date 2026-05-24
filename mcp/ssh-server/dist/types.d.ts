/**
 * Shared type definitions for the SSH MCP server.
 */
/** CLI configuration parsed from command-line arguments */
export interface ServerConfig {
    host: string;
    port: number;
    user: string;
    password: string;
    sudoPassword?: string;
    timeout: number;
    maxChars: number | null;
}
/** Result of a command execution */
export interface ExecResult {
    stdout: string;
    stderr: string;
    exitCode: number;
}
/** Connection state */
export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'failed';
/** Structured log entry written to stderr */
export interface LogEntry {
    ts: string;
    level: 'debug' | 'info' | 'warn' | 'error';
    event: string;
    host?: string;
    user?: string;
    durationMs?: number;
    exitCode?: number;
    attempt?: number;
    error?: string;
}
//# sourceMappingURL=types.d.ts.map