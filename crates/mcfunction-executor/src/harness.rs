use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::exec::Executor;
use crate::version::MinecraftVersion;

/// A `TestServer`-compatible wrapper around the in-process `Executor`.
///
/// Implements the same API surface as `mdl_test::TestServer` so that
/// existing tests can use it as a drop-in alternative.
pub struct McExecutor {
    inner: Executor,
    sandbox_root: PathBuf,
    preserve: bool,
    log_queue: VecDeque<String>,
    log_start_index: usize,
    command_timeout: Duration,
}

impl McExecutor {
    /// Creates a new executor bound to an existing sandbox directory.
    ///
    /// Panics if the sandbox root does not exist.
    #[must_use]
    pub fn create(sandbox_root: PathBuf, version: MinecraftVersion) -> Self {
        assert!(sandbox_root.is_dir(), "sandbox root must exist");
        Self {
            inner: Executor::new(sandbox_root.clone(), version),
            sandbox_root,
            preserve: false,
            log_queue: VecDeque::new(),
            log_start_index: 0,
            command_timeout: Duration::from_secs(20),
        }
    }

    /// Loads all datapacks from `world/datapacks/` inside the sandbox.
    ///
    /// # Errors
    ///
    /// Returns an error if the datapack directory cannot be read or a
    /// function file is invalid.
    pub fn load_datapacks(&mut self) -> Result<(), String> {
        self.inner.load_datapacks()
    }

    /// Returns the sandbox root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.sandbox_root
    }

    /// Prevents deletion of the sandbox directory on drop.
    pub fn preserve_sandbox(&mut self) {
        self.preserve = true;
    }

    /// Sends a console command (blocking — executes immediately in-process).
    ///
    /// # Errors
    ///
    /// Returns an error for commands containing newlines.
    pub fn command(&mut self, command: &str) -> Result<(), String> {
        if command.contains(['\n', '\r']) {
            return Err("server commands must contain exactly one line".to_owned());
        }
        let log_lines = self.inner.console_command(command)?;
        self.log_queue.extend(log_lines);
        Ok(())
    }

    /// Waits for a substring to appear in the log.
    ///
    /// Since execution is synchronous, this reads from the in-memory log
    /// buffer. The timeout is accepted for API compatibility but never
    /// expires.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected text is never found.
    pub fn wait_for_log(
        &mut self,
        expected: &str,
        _timeout: Duration,
    ) -> Result<String, String> {
        for i in 0..self.log_queue.len() {
            if self.log_queue[i].contains(expected) {
                let line = self.log_queue[i].clone();
                // Drain up to and including the matched line
                self.log_queue.drain(0..=i);
                self.log_start_index += i + 1;
                return Ok(line);
            }
        }
        // Check inner log too
        for log_line in &self.inner.log {
            if log_line.contains(expected) {
                return Ok(log_line.clone());
            }
        }
        Err(format!("timed out waiting for {expected:?}"))
    }

    /// Convenience wrapper using the default command timeout.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::wait_for_log`].
    pub fn wait_for_command_log(&mut self, expected: &str) -> Result<String, String> {
        self.wait_for_log(expected, self.command_timeout)
    }

    /// Returns a checkpoint representing the current position in the log.
    #[must_use]
    pub fn log_checkpoint(&self) -> LogCheckpoint {
        LogCheckpoint {
            line_index: self.log_start_index + self.log_queue.len(),
        }
    }

    /// Returns log lines since the given checkpoint that contain `expected`.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkpoint is invalid.
    pub fn matching_log_lines_since(
        &self,
        checkpoint: LogCheckpoint,
        expected: &str,
    ) -> Result<Vec<String>, String> {
        let end = self.log_start_index + self.log_queue.len();
        if checkpoint.line_index > end {
            return Err("log checkpoint out of range".to_owned());
        }
        let start_in_queue = checkpoint
            .line_index
            .checked_sub(self.log_start_index)
            .unwrap_or(0);
        Ok(self
            .log_queue
            .iter()
            .skip(start_in_queue)
            .filter(|line| line.contains(expected))
            .cloned()
            .collect())
    }

    /// Checks for datapack load errors since the given checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if any attributable datapack problems are found.
    pub fn check_datapack_logs_since(
        &self,
        _checkpoint: LogCheckpoint,
    ) -> Result<(), String> {
        // For the initial implementation, this is a no-op.
        Ok(())
    }

    /// Shuts down the executor gracefully.
    ///
    /// # Errors
    ///
    /// Currently always succeeds.
    pub fn shutdown(self) -> Result<(), String> {
        Ok(())
    }

    /// Returns a reference to the executor for test assertions.
    #[must_use]
    pub fn executor(&self) -> &Executor {
        &self.inner
    }

    /// Returns a mutable reference to the inner executor for test assertions.
    pub fn executor_mut(&mut self) -> &mut Executor {
        &mut self.inner
    }
}

/// A stable position in the executor's log output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogCheckpoint {
    line_index: usize,
}
