//! Command executor — runs commands via tokio::process, captures output.
//! In production, output streams back via gRPC.

use kith_common::error::KithError;
use tokio::process::Command;

/// Result of a command execution.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Execute a command and capture its output.
/// This is the simple non-streaming version for basic use.
pub async fn exec_command(command: &str) -> Result<ExecResult, KithError> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
        .map_err(|e| KithError::Internal(format!("failed to execute command: {e}")))?;

    Ok(ExecResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exec_echo() {
        let result = exec_command("echo hello").await.unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn exec_nonzero_exit() {
        let result = exec_command("exit 42").await.unwrap();
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn exec_captures_stderr() {
        let result = exec_command("echo err >&2").await.unwrap();
        assert!(result.stderr.contains("err"));
    }

    #[tokio::test]
    async fn exec_nonexistent_command() {
        let result = exec_command("nonexistent_command_xyz_123").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(!result.stderr.is_empty());
    }

    #[tokio::test]
    async fn exec_pipeline() {
        let result = exec_command("echo 'hello world' | wc -w").await.unwrap();
        assert_eq!(result.stdout.trim(), "2");
        assert_eq!(result.exit_code, 0);
    }
}
