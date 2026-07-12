//! Vanilla Minecraft server harness for MDL integration tests.
//!
//! The harness intentionally uses the ordinary dedicated server instead of mocking
//! command behavior. Fast compiler tests should not use this crate; it is for
//! pack-load and command-execution integration tests.

use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const READY_TEXT: &str = "For help, type \"help\"";
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_MAX_HEAP_MIB: u32 = 1_024;
const SMOKE_MARKER: &str = "MDL_SMOKE_RESULT_42";
const MAX_ERROR_LOG_LINES: usize = 80;

static NEXT_SANDBOX_ID: AtomicU64 = AtomicU64::new(0);

/// Result type used by the server harness.
pub type Result<T> = std::result::Result<T, HarnessError>;

/// Configuration for one dedicated-server process.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Java executable used to start the server.
    pub java: PathBuf,
    /// Official Minecraft dedicated-server JAR.
    pub server_jar: PathBuf,
    /// Maximum Java heap size in MiB.
    pub max_heap_mib: u32,
    /// Time allowed for the server to announce readiness.
    pub startup_timeout: Duration,
    /// Default time allowed for an expected command result to appear in the log.
    pub command_timeout: Duration,
    /// Time allowed for a graceful `stop` before the process is killed.
    pub shutdown_timeout: Duration,
}

impl ServerConfig {
    /// Creates a configuration with conservative local-test defaults.
    #[must_use]
    pub fn new(java: impl Into<PathBuf>, server_jar: impl Into<PathBuf>) -> Self {
        Self {
            java: java.into(),
            server_jar: server_jar.into(),
            max_heap_mib: DEFAULT_MAX_HEAP_MIB,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }

    fn validate(&self) -> Result<()> {
        if !self.server_jar.is_file() {
            return Err(HarnessError::InvalidConfig(format!(
                "server JAR does not exist: {}",
                self.server_jar.display()
            )));
        }
        if self.max_heap_mib == 0 {
            return Err(HarnessError::InvalidConfig(
                "max_heap_mib must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A disposable server directory prepared before the Java process starts.
#[derive(Debug)]
pub struct ServerSandbox {
    root: PathBuf,
    preserve: bool,
}

impl ServerSandbox {
    /// Creates a uniquely named server directory below the system temporary folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the temporary directory or base server files cannot be
    /// created.
    pub fn create(preserve: bool) -> Result<Self> {
        let base = env::temp_dir();
        for _ in 0..100 {
            let id = NEXT_SANDBOX_ID.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = base.join(format!("mdl-test-{}-{nanos}-{id}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => {
                    let sandbox = Self { root, preserve };
                    sandbox.write_server_files()?;
                    return Ok(sandbox);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(HarnessError::io("create test sandbox", error)),
            }
        }
        Err(HarnessError::InvalidConfig(
            "could not allocate a unique test sandbox".to_owned(),
        ))
    }

    /// Root directory used as the server's current working directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Controls whether the sandbox is deleted when the server handle is dropped.
    pub fn set_preserve(&mut self, preserve: bool) {
        self.preserve = preserve;
    }

    /// Writes one file into a datapack installed in the test world.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe relative path or when the file cannot be
    /// written.
    pub fn write_datapack_file(
        &self,
        pack_name: &str,
        relative_path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf> {
        validate_component(pack_name, "datapack name")?;
        let relative_path = relative_path.as_ref();
        validate_relative_path(relative_path)?;
        let path = self
            .root
            .join("world/datapacks")
            .join(pack_name)
            .join(relative_path);
        write_file(&path, contents.as_ref())?;
        Ok(path)
    }

    /// Starts the server and waits for its normal ready message.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration is invalid, Java cannot be started, the
    /// server exits early, or startup times out.
    pub fn start(self, config: &ServerConfig) -> Result<TestServer> {
        config.validate()?;

        let log_file = File::create(self.root.join("harness.log"))
            .map_err(|error| HarnessError::io("create server log", error))?;
        let shared_log_file = Arc::new(Mutex::new(log_file));
        let mut command = Command::new(&config.java);
        command
            .current_dir(&self.root)
            .arg(format!("-Xms{}M", config.max_heap_mib.min(256)))
            .arg(format!("-Xmx{}M", config.max_heap_mib))
            .arg("-jar")
            .arg(&config.server_jar)
            .arg("--nogui")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|error| HarnessError::io("start Minecraft server", error))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            HarnessError::InvalidConfig("server process did not expose stdin".to_owned())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            HarnessError::InvalidConfig("server process did not expose stdout".to_owned())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            HarnessError::InvalidConfig("server process did not expose stderr".to_owned())
        })?;

        let (line_tx, line_rx) = mpsc::channel();
        let stdout_thread = spawn_log_reader(
            "stdout",
            stdout,
            line_tx.clone(),
            Arc::clone(&shared_log_file),
        );
        let stderr_thread =
            spawn_log_reader("stderr", stderr, line_tx, Arc::clone(&shared_log_file));

        let mut server = TestServer {
            child: Some(child),
            stdin: Some(stdin),
            line_rx,
            log_lines: Vec::new(),
            reader_threads: vec![stdout_thread, stderr_thread],
            sandbox: self,
            shutdown_timeout: config.shutdown_timeout,
            command_timeout: config.command_timeout,
        };

        if let Err(error) = server.wait_for_log(READY_TEXT, config.startup_timeout) {
            return Err(server.preserve_failure(error));
        }
        Ok(server)
    }

    fn write_server_files(&self) -> Result<()> {
        write_file(&self.root.join("eula.txt"), b"eula=true\n")?;
        write_file(
            &self.root.join("server.properties"),
            concat!(
                "allow-flight=true\n",
                "difficulty=peaceful\n",
                "enable-query=false\n",
                "enable-rcon=false\n",
                "enable-status=false\n",
                "generate-structures=false\n",
                "generator-settings={\"biome\":\"minecraft:plains\",\"features\":false,\"lakes\":false,\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},{\"block\":\"minecraft:dirt\",\"height\":2},{\"block\":\"minecraft:grass_block\",\"height\":1}]}\n",
                "level-name=world\n",
                "level-type=minecraft:flat\n",
                "max-players=1\n",
                "max-tick-time=-1\n",
                "online-mode=false\n",
                "pause-when-empty-seconds=-1\n",
                "server-ip=127.0.0.1\n",
                "server-port=0\n",
                "simulation-distance=2\n",
                "spawn-monsters=false\n",
                "spawn-protection=0\n",
                "sync-chunk-writes=false\n",
                "view-distance=2\n",
            )
            .as_bytes(),
        )?;
        Ok(())
    }
}

impl Drop for ServerSandbox {
    fn drop(&mut self) {
        if !self.preserve {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

/// A running dedicated server with line-oriented command and log access.
#[derive(Debug)]
pub struct TestServer {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    line_rx: Receiver<LogLine>,
    log_lines: Vec<LogLine>,
    reader_threads: Vec<JoinHandle<()>>,
    sandbox: ServerSandbox,
    shutdown_timeout: Duration,
    command_timeout: Duration,
}

impl TestServer {
    /// Directory containing the temporary world and `harness.log`.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.sandbox.root()
    }

    /// Prevents deletion of this server directory on drop.
    pub fn preserve_sandbox(&mut self) {
        self.sandbox.set_preserve(true);
    }

    /// Sends a console command followed by a newline.
    ///
    /// # Errors
    ///
    /// Returns an error if the command contains a newline, the server is not
    /// running, or its standard input cannot be written.
    pub fn command(&mut self, command: &str) -> Result<()> {
        validate_command_line(command)?;
        let stdin = self.stdin.as_mut().ok_or(HarnessError::NotRunning)?;
        stdin
            .write_all(command.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush())
            .map_err(|error| HarnessError::io("write server command", error))
    }

    /// Waits for a substring to occur in a newly received server output line.
    ///
    /// # Errors
    ///
    /// Returns an error if the server exits, closes its output, cannot be polled, or
    /// the expected text does not appear before the timeout.
    pub fn wait_for_log(&mut self, expected: &str, timeout: Duration) -> Result<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Err(HarnessError::ProcessExited {
                    status,
                    log: self.recent_log(),
                });
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(HarnessError::Timeout {
                    expected: expected.to_owned(),
                    timeout,
                    log: self.recent_log(),
                });
            }
            let remaining = deadline.saturating_duration_since(now);
            match self
                .line_rx
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(line) => {
                    let matched = line.text.contains(expected);
                    let text = line.text.clone();
                    self.log_lines.push(line);
                    if matched {
                        return Ok(text);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if let Some(status) = self.try_wait()? {
                        return Err(HarnessError::ProcessExited {
                            status,
                            log: self.recent_log(),
                        });
                    }
                    return Err(HarnessError::LogClosed {
                        log: self.recent_log(),
                    });
                }
            }
        }
    }

    /// Uses the configured command timeout when waiting for a log substring.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::wait_for_log`].
    pub fn wait_for_command_log(&mut self, expected: &str) -> Result<String> {
        self.wait_for_log(expected, self.command_timeout)
    }

    /// Returns recent output collected by waits performed so far.
    #[must_use]
    pub fn recent_log(&self) -> String {
        self.log_lines
            .iter()
            .rev()
            .take(MAX_ERROR_LOG_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|line| format!("[{}] {}", line.stream, line.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Gracefully stops the server. Drop provides the same cleanup as a fallback.
    ///
    /// # Errors
    ///
    /// Returns an error if the child process cannot be polled, killed, or reaped.
    pub fn shutdown(mut self) -> Result<()> {
        self.shutdown_inner()
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or(HarnessError::NotRunning)?
            .try_wait()
            .map_err(|error| HarnessError::io("poll Minecraft server", error))
    }

    fn preserve_failure(&mut self, error: HarnessError) -> HarnessError {
        let sandbox = self.root().to_path_buf();
        let context = self.recent_log();
        self.preserve_sandbox();
        HarnessError::PreservedFailure {
            sandbox,
            source: Box::new(error.with_log(context)),
        }
    }

    fn shutdown_inner(&mut self) -> Result<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };

        if child
            .try_wait()
            .map_err(|error| HarnessError::io("poll Minecraft server during shutdown", error))?
            .is_none()
        {
            if let Some(stdin) = &mut self.stdin {
                let _ = stdin.write_all(b"stop\n");
                let _ = stdin.flush();
            }
            self.stdin.take();

            let deadline = Instant::now() + self.shutdown_timeout;
            while Instant::now() < deadline {
                if child
                    .try_wait()
                    .map_err(|error| HarnessError::io("wait for Minecraft shutdown", error))?
                    .is_some()
                {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            if child
                .try_wait()
                .map_err(|error| HarnessError::io("poll Minecraft shutdown", error))?
                .is_none()
            {
                child
                    .kill()
                    .map_err(|error| HarnessError::io("kill Minecraft server", error))?;
                child
                    .wait()
                    .map_err(|error| HarnessError::io("reap Minecraft server", error))?;
            }
        }

        self.child.take();
        for reader in self.reader_threads.drain(..) {
            let _ = reader.join();
        }
        Ok(())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

/// Generates and executes the Stage 1 smoke datapack.
///
/// # Errors
///
/// The returned path is present only when `preserve` is true. Failed server runs are
/// always preserved and include their path in the error.
///
/// Returns an error if the sandbox or datapack cannot be created, the server cannot
/// start, or the expected smoke marker is not produced.
pub fn run_smoke_test(config: &ServerConfig, preserve: bool) -> Result<Option<PathBuf>> {
    let sandbox = ServerSandbox::create(preserve)?;
    install_smoke_pack(&sandbox)?;
    let mut server = sandbox.start(config)?;
    let root = server.root().to_path_buf();
    if let Err(error) = server.command("function mdl_test:smoke") {
        return Err(server.preserve_failure(error));
    }
    if let Err(error) = server.wait_for_command_log(SMOKE_MARKER) {
        return Err(server.preserve_failure(error));
    }
    server.shutdown()?;
    Ok(preserve.then_some(root))
}

/// Installs the tiny datapack used to prove pack loading and command execution.
///
/// # Errors
///
/// Returns an error if either datapack file cannot be written.
pub fn install_smoke_pack(sandbox: &ServerSandbox) -> Result<()> {
    sandbox.write_datapack_file(
        "mdl_smoke",
        "pack.mcmeta",
        concat!(
            "{\n",
            "  \"pack\": {\n",
            "    \"description\": \"MDL vanilla harness smoke test\",\n",
            "    \"min_format\": [107, 1],\n",
            "    \"max_format\": [107, 1]\n",
            "  }\n",
            "}\n",
        ),
    )?;
    sandbox.write_datapack_file(
        "mdl_smoke",
        "data/mdl_test/function/smoke.mcfunction",
        concat!(
            "scoreboard objectives add mdl_test dummy\n",
            "scoreboard players set #result mdl_test 42\n",
            "execute if score #result mdl_test matches 42 run say MDL_SMOKE_RESULT_42\n",
        ),
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
struct LogLine {
    stream: &'static str,
    text: String,
}

fn spawn_log_reader<R>(
    stream: &'static str,
    reader: R,
    line_tx: mpsc::Sender<LogLine>,
    log_file: Arc<Mutex<File>>,
) -> JoinHandle<()>
where
    R: io::Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else {
                break;
            };
            if let Ok(mut file) = log_file.lock() {
                let _ = writeln!(file, "[{stream}] {line}");
                let _ = file.flush();
            }
            if line_tx.send(LogLine { stream, text: line }).is_err() {
                break;
            }
        }
    })
}

fn validate_component(value: &str, description: &str) -> Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(HarnessError::InvalidConfig(format!(
            "invalid {description}: {value:?}"
        )));
    }
    Ok(())
}

fn validate_command_line(command: &str) -> Result<()> {
    if command.contains(['\n', '\r']) {
        return Err(HarnessError::InvalidConfig(
            "server commands must contain exactly one line".to_owned(),
        ));
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(HarnessError::InvalidConfig(format!(
            "datapack path must be a non-empty relative path without '..': {}",
            path.display()
        )));
    }
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        HarnessError::InvalidConfig(format!("file has no parent: {}", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| HarnessError::io("create directory", error))?;
    fs::write(path, contents).map_err(|error| HarnessError::io("write test file", error))
}

/// Failures include recent server output where it helps diagnose the problem.
#[derive(Debug)]
pub enum HarnessError {
    /// Invalid path, option, or configuration value.
    InvalidConfig(String),
    /// Filesystem or process I/O failed.
    Io {
        context: &'static str,
        source: io::Error,
    },
    /// The expected server output did not appear in time.
    Timeout {
        expected: String,
        timeout: Duration,
        log: String,
    },
    /// The server exited before the operation completed.
    ProcessExited { status: ExitStatus, log: String },
    /// The server output streams closed unexpectedly.
    LogClosed { log: String },
    /// An operation was attempted after process shutdown.
    NotRunning,
    /// A failed server sandbox was retained for inspection.
    PreservedFailure { sandbox: PathBuf, source: Box<Self> },
}

impl HarnessError {
    fn io(context: &'static str, source: io::Error) -> Self {
        Self::Io { context, source }
    }

    fn with_log(self, fallback_log: String) -> Self {
        match self {
            Self::Timeout {
                expected,
                timeout,
                log,
            } => Self::Timeout {
                expected,
                timeout,
                log: merge_log(log, fallback_log),
            },
            Self::ProcessExited { status, log } => Self::ProcessExited {
                status,
                log: merge_log(log, fallback_log),
            },
            Self::LogClosed { log } => Self::LogClosed {
                log: merge_log(log, fallback_log),
            },
            other => other,
        }
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid configuration: {message}"),
            Self::Io { context, source } => write!(formatter, "{context}: {source}"),
            Self::Timeout {
                expected,
                timeout,
                log,
            } => write!(
                formatter,
                "timed out after {timeout:?} waiting for {expected:?}{}",
                display_log(log)
            ),
            Self::ProcessExited { status, log } => write!(
                formatter,
                "Minecraft server exited early with {status}{}",
                display_log(log)
            ),
            Self::LogClosed { log } => write!(
                formatter,
                "Minecraft server closed its output streams{}",
                display_log(log)
            ),
            Self::NotRunning => write!(formatter, "Minecraft server is not running"),
            Self::PreservedFailure { sandbox, source } => write!(
                formatter,
                "{source}\nfailed server sandbox preserved at {}",
                sandbox.display()
            ),
        }
    }
}

impl StdError for HarnessError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::PreservedFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn merge_log(current: String, fallback: String) -> String {
    if current.is_empty() {
        fallback
    } else {
        current
    }
}

fn display_log(log: &str) -> String {
    if log.is_empty() {
        String::new()
    } else {
        format!("\nrecent server output:\n{log}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_has_server_configuration_and_cleans_itself() {
        let root;
        {
            let sandbox = ServerSandbox::create(false).expect("create sandbox");
            root = sandbox.root().to_path_buf();
            assert!(root.join("eula.txt").is_file());
            let properties =
                fs::read_to_string(root.join("server.properties")).expect("read server properties");
            assert!(properties.contains("level-name=world\n"));
            assert!(properties.contains("server-port=0\n"));
        }
        assert!(!root.exists());
    }

    #[test]
    fn smoke_pack_uses_the_26_2_layout() {
        let sandbox = ServerSandbox::create(false).expect("create sandbox");
        install_smoke_pack(&sandbox).expect("install smoke pack");
        let pack = sandbox.root().join("world/datapacks/mdl_smoke");
        let metadata = fs::read_to_string(pack.join("pack.mcmeta")).expect("read metadata");
        assert!(metadata.contains("\"min_format\": [107, 1]"));
        assert!(
            pack.join("data/mdl_test/function/smoke.mcfunction")
                .is_file()
        );
    }

    #[test]
    fn datapack_paths_cannot_escape_the_sandbox() {
        let sandbox = ServerSandbox::create(false).expect("create sandbox");
        let error = sandbox
            .write_datapack_file("mdl_smoke", "../outside", b"bad")
            .expect_err("parent traversal must fail");
        assert!(error.to_string().contains("relative path"));
    }

    #[test]
    fn multiline_console_commands_are_rejected() {
        let error = validate_command_line("one\ntwo").expect_err("newlines must fail");
        assert!(error.to_string().contains("exactly one line"));
        validate_command_line("one command").expect("one line is valid");
    }
}
