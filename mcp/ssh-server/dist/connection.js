/**
 * SSH Connection Manager
 *
 * Resilient SSH connection with:
 * - Exponential backoff reconnection (3s → 6s → 12s → 24s → 30s, max 5 attempts)
 * - SSH keepalive every 10 seconds
 * - Health check before each command
 * - exec channels only (no persistent shells — avoids PTY accumulation)
 * - Sudo via stdin pipe
 * - Structured stderr logging
 */
import { Client } from 'ssh2';
import { sanitizeError } from './sanitize.js';
const RECONNECT_DELAYS_MS = [3000, 6000, 12000, 24000, 30000];
const KEEPALIVE_INTERVAL_MS = 10_000;
const KEEPALIVE_COUNT_MAX = 3;
function log(entry) {
    const full = { ts: new Date().toISOString(), ...entry };
    process.stderr.write(JSON.stringify(full) + '\n');
}
export class SSHConnectionManager {
    client = null;
    state = 'disconnected';
    connectPromise = null;
    _failedAt = null;
    _bgRetryTimer = null;
    config;
    constructor(config) {
        this.config = config;
    }
    // ─── Connection lifecycle ────────────────────────────────────────────────
    buildConnectConfig() {
        return {
            host: this.config.host,
            port: this.config.port,
            username: this.config.user,
            password: this.config.password,
            readyTimeout: 10_000,
            keepaliveInterval: KEEPALIVE_INTERVAL_MS,
            keepaliveCountMax: KEEPALIVE_COUNT_MAX,
        };
    }
    connectOnce() {
        return new Promise((resolve, reject) => {
            const client = new Client();
            client.on('ready', () => {
                this.client = client;
                this.state = 'connected';
                log({ level: 'info', event: 'ssh_connected', host: this.config.host, user: this.config.user });
                resolve();
            });
            client.on('error', (err) => {
                log({ level: 'error', event: 'ssh_error', host: this.config.host, error: sanitizeError(err) });
                reject(err);
            });
            client.on('close', () => {
                // Guard by client identity — a late 'close' from an old client
                // must not wipe the new client created during reconnect.
                if (this.client === client) {
                    this.state = 'disconnected';
                    this.client = null;
                    log({ level: 'warn', event: 'ssh_closed', host: this.config.host });
                }
            });
            client.on('end', () => {
                if (this.client === client) {
                    this.state = 'disconnected';
                    this.client = null;
                    log({ level: 'warn', event: 'ssh_ended', host: this.config.host });
                }
            });
            client.connect(this.buildConnectConfig());
        });
    }
    async connect() {
        if (this.state === 'connected' && this.client)
            return;
        // Deduplicate concurrent connect calls
        if (this.connectPromise) {
            return this.connectPromise;
        }
        this.state = 'connecting';
        this.connectPromise = this._connectWithRetry().finally(() => {
            this.connectPromise = null;
        });
        return this.connectPromise;
    }
    async _connectWithRetry() {
        for (let attempt = 0; attempt <= RECONNECT_DELAYS_MS.length; attempt++) {
            try {
                await this.connectOnce();
                return;
            }
            catch (err) {
                const isLast = attempt >= RECONNECT_DELAYS_MS.length;
                if (isLast) {
                    this.state = 'failed';
                    this._failedAt = Date.now();
                    this._scheduleBackgroundRetry();
                    throw new Error(`SSH connection failed after ${attempt + 1} attempts: ${sanitizeError(err)}`);
                }
                const delay = RECONNECT_DELAYS_MS[attempt];
                log({
                    level: 'warn',
                    event: 'ssh_reconnecting',
                    host: this.config.host,
                    attempt: attempt + 1,
                    error: sanitizeError(err),
                });
                await sleep(delay);
                this.state = 'reconnecting';
            }
        }
    }
    /** Ensure we have a live connection before executing a command. */
    async ensureConnected() {
        if (this.state !== 'connected' || !this.client) {
            await this.connect();
        }
        if (!this.client) {
            throw new Error('SSH connection unavailable');
        }
        return this.client;
    }
    async close() {
        if (this._bgRetryTimer) {
            clearTimeout(this._bgRetryTimer);
            this._bgRetryTimer = null;
        }
        if (this.client) {
            this.state = 'disconnected';
            this.client.end();
            this.client = null;
            log({ level: 'info', event: 'ssh_disconnected', host: this.config.host });
        }
    }
    /** Schedule a background reconnect attempt 30s after entering 'failed' state. */
    _scheduleBackgroundRetry() {
        if (this._bgRetryTimer) return; // already scheduled
        this._bgRetryTimer = setTimeout(async () => {
            this._bgRetryTimer = null;
            if (this.state !== 'failed') return; // already recovered
            log({ level: 'info', event: 'ssh_bg_retry', host: this.config.host });
            try {
                this.state = 'disconnected'; // allow connect() to run
                await this.connect();
            }
            catch (_err) {
                // connect() already logged; if still failed it will reschedule itself
            }
        }, 30_000);
    }
    // ─── Command execution ───────────────────────────────────────────────────
    /**
     * Execute a command as the configured user.
     * Uses exec channels (not persistent shells) to avoid PTY accumulation.
     */
    async exec(command) {
        const client = await this.ensureConnected();
        const start = Date.now();
        let timeoutId;
        const timeoutPromise = new Promise((_, reject) => {
            timeoutId = setTimeout(() => {
                this.state = 'disconnected';
                this.client = null;
                reject(new Error(`Command timed out after ${this.config.timeout}ms`));
            }, this.config.timeout);
        });
        const execPromise = new Promise((resolve, reject) => {
            client.exec(command, (err, stream) => {
                if (err) {
                    clearTimeout(timeoutId);
                    // Connection may be broken — reset state so next call reconnects
                    this.state = 'disconnected';
                    this.client = null;
                    reject(new Error(`exec failed: ${sanitizeError(err)}`));
                    return;
                }
                const stdoutChunks = [];
                const stderrChunks = [];
                stream.on('data', (chunk) => stdoutChunks.push(chunk));
                stream.stderr.on('data', (chunk) => stderrChunks.push(chunk));
                stream.on('close', (code) => {
                    clearTimeout(timeoutId);
                    const exitCode = code ?? -1;
                    let stdout = Buffer.concat(stdoutChunks).toString('utf8');
                    let stderr = Buffer.concat(stderrChunks).toString('utf8');
                    // Apply maxChars limit
                    if (this.config.maxChars !== null) {
                        stdout = stdout.slice(0, this.config.maxChars);
                        stderr = stderr.slice(0, this.config.maxChars);
                    }
                    log({
                        level: 'info',
                        event: 'exec_done',
                        host: this.config.host,
                        durationMs: Date.now() - start,
                        exitCode,
                    });
                    resolve({ stdout, stderr, exitCode });
                });
                stream.on('error', (err) => {
                    clearTimeout(timeoutId);
                    this.state = 'disconnected';
                    this.client = null;
                    reject(new Error(`stream error: ${sanitizeError(err)}`));
                });
            });
        });
        return Promise.race([execPromise, timeoutPromise]);
    }
    /**
     * Execute a command with sudo elevation.
     * Pipes the sudo password via stdin — never uses a persistent shell.
     *
     * Strategy: `echo '<password>' | sudo -S -p '' <command>`
     * The `-S` flag reads password from stdin; `-p ''` suppresses the prompt.
     */
    async sudoExec(command) {
        if (!this.config.sudoPassword) {
            throw new Error('sudoPassword not configured');
        }
        // Build the sudo command — password is piped via stdin, not shell-interpolated
        // We use printf to avoid echo adding a trailing newline issue on some systems
        const sudoCmd = `sudo -S -p '' ${command}`;
        const client = await this.ensureConnected();
        const start = Date.now();
        let timeoutId;
        const timeoutPromise = new Promise((_, reject) => {
            timeoutId = setTimeout(() => {
                this.state = 'disconnected';
                this.client = null;
                reject(new Error(`sudo command timed out after ${this.config.timeout}ms`));
            }, this.config.timeout);
        });
        const execPromise = new Promise((resolve, reject) => {
            client.exec(sudoCmd, (err, stream) => {
                if (err) {
                    clearTimeout(timeoutId);
                    this.state = 'disconnected';
                    this.client = null;
                    reject(new Error(`sudo exec failed: ${sanitizeError(err)}`));
                    return;
                }
                const stdoutChunks = [];
                const stderrChunks = [];
                stream.on('data', (chunk) => stdoutChunks.push(chunk));
                stream.stderr.on('data', (chunk) => stderrChunks.push(chunk));
                stream.on('close', (code) => {
                    clearTimeout(timeoutId);
                    const exitCode = code ?? -1;
                    let stdout = Buffer.concat(stdoutChunks).toString('utf8');
                    let stderr = Buffer.concat(stderrChunks).toString('utf8');
                    // Strip sudo password prompt artifacts from stderr
                    stderr = stderr.replace(/^\[sudo\] password.*?\n/m, '');
                    // Apply maxChars limit
                    if (this.config.maxChars !== null) {
                        stdout = stdout.slice(0, this.config.maxChars);
                        stderr = stderr.slice(0, this.config.maxChars);
                    }
                    log({
                        level: 'info',
                        event: 'sudo_exec_done',
                        host: this.config.host,
                        durationMs: Date.now() - start,
                        exitCode,
                    });
                    resolve({ stdout, stderr, exitCode });
                });
                stream.on('error', (err) => {
                    clearTimeout(timeoutId);
                    this.state = 'disconnected';
                    this.client = null;
                    reject(new Error(`sudo stream error: ${sanitizeError(err)}`));
                });
                // Write the sudo password to stdin, then close stdin
                stream.stdin.write(this.config.sudoPassword + '\n');
                stream.stdin.end();
            });
        });
        return Promise.race([execPromise, timeoutPromise]);
    }
}
// ─── Utilities ──────────────────────────────────────────────────────────────
function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
//# sourceMappingURL=connection.js.map