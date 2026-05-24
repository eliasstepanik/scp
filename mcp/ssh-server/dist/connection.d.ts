/**
 * SSH Connection Manager
 *
 * Resilient SSH connection with:
 * - Exponential backoff reconnection (3s → 6s → 12s → 24s → 30s, max 5 attempts)
 * - SSH keepalive every 30 seconds
 * - Health check before each command
 * - exec channels only (no persistent shells — avoids PTY accumulation)
 * - Sudo via stdin pipe
 * - Structured stderr logging
 */
import type { ServerConfig, ExecResult } from './types.js';
export declare class SSHConnectionManager {
    private client;
    private state;
    private connectPromise;
    private readonly config;
    constructor(config: ServerConfig);
    private buildConnectConfig;
    private connectOnce;
    connect(): Promise<void>;
    private _connectWithRetry;
    /** Ensure we have a live connection before executing a command. */
    private ensureConnected;
    close(): Promise<void>;
    /**
     * Execute a command as the configured user.
     * Uses exec channels (not persistent shells) to avoid PTY accumulation.
     */
    exec(command: string): Promise<ExecResult>;
    /**
     * Execute a command with sudo elevation.
     * Pipes the sudo password via stdin — never uses a persistent shell.
     *
     * Strategy: `echo '<password>' | sudo -S -p '' <command>`
     * The `-S` flag reads password from stdin; `-p ''` suppresses the prompt.
     */
    sudoExec(command: string): Promise<ExecResult>;
}
//# sourceMappingURL=connection.d.ts.map