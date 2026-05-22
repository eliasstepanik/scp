use crate::error::TransportError;
use scp_core::protocol::IncomingMessage;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Server-facing stdio transport (spawns a child process)
pub struct StdioServerTransport {
    #[allow(dead_code)]
    child: Child,
    stdin_tx: mpsc::Sender<String>,
    stdout_rx: mpsc::Receiver<String>,
    _stdin_task: tokio::task::JoinHandle<()>,
    _stdout_task: tokio::task::JoinHandle<()>,
    _stderr_task: tokio::task::JoinHandle<()>,
}

impl StdioServerTransport {
    /// Spawn a child process and set up stdio transport
    pub async fn spawn(
        command: &str,
        args: &[&str],
        env: &HashMap<String, String>,
    ) -> Result<Self, TransportError> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| TransportError::ProcessError(format!("Failed to spawn process: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TransportError::ProcessError("Failed to get stdin".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TransportError::ProcessError("Failed to get stdout".to_string()))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TransportError::ProcessError("Failed to get stderr".to_string()))?;

        let (stdin_tx, stdin_rx) = mpsc::channel::<String>(100);
        let (stdout_tx, stdout_rx) = mpsc::channel::<String>(100);

        let stdin_task = tokio::spawn(stdin_writer_task(stdin, stdin_rx));
        let stdout_task = tokio::spawn(stdout_reader_task(stdout, stdout_tx));
        let stderr_task = tokio::spawn(stderr_logger_task(stderr, command.to_string()));

        Ok(Self {
            child,
            stdin_tx,
            stdout_rx,
            _stdin_task: stdin_task,
            _stdout_task: stdout_task,
            _stderr_task: stderr_task,
        })
    }

    /// Send a JSON-RPC message to the child process
    pub async fn send(&self, msg: &Value) -> Result<(), TransportError> {
        let json_str = serde_json::to_string(msg)?;
        self.stdin_tx
            .send(json_str)
            .await
            .map_err(|_| TransportError::ConnectionClosed)?;
        Ok(())
    }

    /// Receive a JSON-RPC message from the child process
    pub async fn receive(&mut self) -> Result<Option<IncomingMessage>, TransportError> {
        match self.stdout_rx.recv().await {
            Some(line) => {
                let msg: IncomingMessage = serde_json::from_str(&line)?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }
}

async fn stdin_writer_task(mut stdin: ChildStdin, mut rx: mpsc::Receiver<String>) {
    while let Some(line) = rx.recv().await {
        if let Err(e) = stdin.write_all(line.as_bytes()).await {
            error!("Failed to write to stdin: {}", e);
            break;
        }
        if let Err(e) = stdin.write_all(b"\n").await {
            error!("Failed to write newline: {}", e);
            break;
        }
        if let Err(e) = stdin.flush().await {
            error!("Failed to flush stdin: {}", e);
            break;
        }
    }
}

async fn stdout_reader_task(stdout: ChildStdout, tx: mpsc::Sender<String>) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() && tx.send(trimmed).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                error!("Failed to read from stdout: {}", e);
                break;
            }
        }
    }
}

async fn stderr_logger_task(stderr: tokio::process::ChildStderr, server_name: String) {
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    debug!(server = %server_name, "server stderr: {}", trimmed);
                }
            }
            Err(e) => {
                debug!(server = %server_name, "Failed to read stderr: {}", e);
                break;
            }
        }
    }
}
