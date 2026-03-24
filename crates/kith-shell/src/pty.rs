//! PTY shell — fork bash via pseudo-terminal.
//! Cross-platform: works on macOS + Linux via nix crate.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::pty::{OpenptyResult, openpty};
use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, Pid, close, dup2, fork, setsid};
use tracing::info;

use kith_common::error::KithError;

/// A PTY-backed bash shell. Forks a child bash process connected
/// via a pseudo-terminal master/slave pair.
pub struct PtyShell {
    master: OwnedFd,
    child_pid: Pid,
}

impl PtyShell {
    /// Spawn a new bash shell connected via PTY.
    pub fn spawn() -> Result<Self, KithError> {
        let OpenptyResult { master, slave } =
            openpty(None, None).map_err(|e| KithError::Internal(format!("openpty failed: {e}")))?;

        // SAFETY: fork is inherently unsafe. We immediately exec in the child
        // and only touch async-signal-safe functions before exec.
        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                // Child: set up slave as stdin/stdout/stderr, exec bash
                drop(master); // close master in child

                let slave_fd = slave.as_raw_fd();

                // New session (detach from parent terminal)
                setsid().ok();

                // Wire slave to stdio
                dup2(slave_fd, 0).ok(); // stdin
                dup2(slave_fd, 1).ok(); // stdout
                dup2(slave_fd, 2).ok(); // stderr

                if slave_fd > 2 {
                    close(slave_fd).ok();
                }

                // Exec bash
                let err = Command::new("/bin/bash")
                    .arg("--norc")
                    .arg("--noprofile")
                    .env("TERM", "xterm-256color")
                    .env("PS1", "") // suppress bash prompt — kith shows its own
                    .exec(); // never returns on success

                // If exec fails
                eprintln!("exec bash failed: {err}");
                std::process::exit(1);
            }
            Ok(ForkResult::Parent { child }) => {
                drop(slave); // close slave in parent

                info!(child_pid = child.as_raw(), "PTY shell spawned");

                Ok(PtyShell {
                    master,
                    child_pid: child,
                })
            }
            Err(e) => Err(KithError::Internal(format!("fork failed: {e}"))),
        }
    }

    /// Write data to the PTY (bash's stdin).
    pub fn write_all(&self, data: &[u8]) -> Result<(), KithError> {
        let mut file = unsafe { std::fs::File::from_raw_fd(self.master.as_raw_fd()) };
        let result = file.write_all(data);
        std::mem::forget(file); // don't close the fd
        result.map_err(|e| KithError::Internal(format!("PTY write failed: {e}")))
    }

    /// Read output from the PTY (bash's stdout/stderr).
    /// Blocking — call from spawn_blocking for async.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, KithError> {
        let mut file = unsafe { std::fs::File::from_raw_fd(self.master.as_raw_fd()) };
        let result = file.read(buf);
        std::mem::forget(file); // don't close the fd
        result.map_err(|e| KithError::Internal(format!("PTY read failed: {e}")))
    }

    /// Read output with a timeout. Returns Ok(0) on timeout.
    pub fn read_with_timeout(
        &self,
        buf: &mut [u8],
        timeout: std::time::Duration,
    ) -> Result<usize, KithError> {
        use nix::poll::{PollFd, PollFlags, PollTimeout, poll};

        use std::os::fd::BorrowedFd;
        let fd = unsafe { BorrowedFd::borrow_raw(self.master.as_raw_fd()) };
        let mut pollfd = [PollFd::new(fd, PollFlags::POLLIN)];
        let timeout_ms =
            PollTimeout::try_from(timeout.as_millis() as u32).unwrap_or(PollTimeout::MAX);

        match poll(&mut pollfd, timeout_ms) {
            Ok(0) => Ok(0), // timeout
            Ok(_) => self.read(buf),
            Err(e) => Err(KithError::Internal(format!("poll failed: {e}"))),
        }
    }

    /// Execute a command via PTY and capture output.
    /// Writes the command, reads output until idle (timeout-based).
    pub fn exec_and_capture(
        &self,
        command: &str,
        timeout: std::time::Duration,
    ) -> Result<String, KithError> {
        // Write command
        self.write_all(format!("{command}\n").as_bytes())?;

        // Read output until timeout (no more data)
        let mut output = String::new();
        let mut buf = [0u8; 4096];

        loop {
            match self.read_with_timeout(&mut buf, timeout) {
                Ok(0) => break, // timeout — no more output
                Ok(n) => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
                Err(e) => {
                    if output.is_empty() {
                        return Err(e);
                    }
                    break; // got some output, read error is probably EOF
                }
            }
        }

        Ok(output)
    }

    /// Check if the child process is still running.
    pub fn is_alive(&self) -> bool {
        matches!(
            waitpid(self.child_pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
            Ok(nix::sys::wait::WaitStatus::StillAlive)
        )
    }

    /// Get the child PID.
    pub fn pid(&self) -> Pid {
        self.child_pid
    }
}

impl Drop for PtyShell {
    fn drop(&mut self) {
        // Send SIGHUP to child
        let _ = nix::sys::signal::kill(self.child_pid, nix::sys::signal::Signal::SIGHUP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_exec() {
        let pty = PtyShell::spawn().expect("should spawn PTY");
        assert!(pty.is_alive(), "child should be alive");

        let output = pty
            .exec_and_capture(
                "echo pty-test-output",
                std::time::Duration::from_millis(500),
            )
            .expect("should capture output");

        assert!(
            output.contains("pty-test-output"),
            "output should contain echo text, got: {output}"
        );
    }

    #[test]
    fn spawn_and_drop() {
        let pid = {
            let pty = PtyShell::spawn().expect("should spawn PTY");
            assert!(pty.is_alive(), "child should be alive after spawn");
            pty.pid()
            // pty dropped here — sends SIGHUP
        };

        std::thread::sleep(std::time::Duration::from_millis(500));

        // After drop, SIGHUP should have killed the child
        let status = waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG));
        assert!(
            !matches!(status, Ok(nix::sys::wait::WaitStatus::StillAlive)),
            "child should not be alive after drop"
        );
    }

    #[test]
    fn exec_multiple_commands() {
        let pty = PtyShell::spawn().expect("should spawn PTY");

        let out1 = pty
            .exec_and_capture("echo first", std::time::Duration::from_millis(500))
            .unwrap();
        assert!(out1.contains("first"));

        let out2 = pty
            .exec_and_capture("echo second", std::time::Duration::from_millis(500))
            .unwrap();
        assert!(out2.contains("second"));
    }

    #[test]
    fn exec_with_exit_code() {
        let pty = PtyShell::spawn().expect("should spawn PTY");

        let output = pty
            .exec_and_capture("echo $?", std::time::Duration::from_millis(500))
            .unwrap();
        // First command's exit code should be 0
        assert!(output.contains("0"), "exit code should be 0, got: {output}");
    }

    #[test]
    fn exec_pipeline() {
        let pty = PtyShell::spawn().expect("should spawn PTY");

        let output = pty
            .exec_and_capture(
                "echo 'hello world' | wc -w",
                std::time::Duration::from_millis(500),
            )
            .unwrap();
        assert!(
            output.contains("2"),
            "word count should be 2, got: {output}"
        );
    }
}
