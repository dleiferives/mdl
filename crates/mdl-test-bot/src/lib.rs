//! Azalea-backed bot client for driving a real player against the pinned Minecraft
//! server harness in `mdl-test`.
//!
//! Deliberately its own, non-workspace-member crate -- see `Cargo.toml` for why.

use std::fmt;
use std::future::Future;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use azalea::BlockPos;
use azalea::prelude::*;
use mdl_test::TestServer;
use tokio::sync::oneshot;

const DEFAULT_SPAWN_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_CONTAINER_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

/// Result type used by this crate's bot client.
pub type Result<T> = std::result::Result<T, BotError>;

/// Failures specific to driving a real bot connection.
#[derive(Debug)]
pub enum BotError {
    /// The bot never reached [`azalea::prelude::Event::Spawn`] within the timeout.
    ///
    /// Per Azalea's own documented behavior, `ClientBuilder::start` retries
    /// forever and will not return on its own -- the background connection thread
    /// is left running (not joined) rather than blocking indefinitely here too.
    SpawnTimeout,
    /// The spawn-relay channel closed without ever sending a client, which only
    /// happens if the background connection thread panicked before reaching
    /// `Event::Spawn`.
    SpawnChannelClosed,
    /// The background connection thread panicked.
    BotThreadPanicked,
    /// The background connection thread did not finish within the shutdown
    /// timeout after `exit()`, even with auto-reconnect disabled. Left running
    /// (not joined) rather than blocking the caller forever -- a bounded,
    /// reported failure beats a silent hang, the same principle
    /// [`BotError::ContainerOpenTimeout`] already applies.
    ShutdownTimeout,
    /// An Azalea operation (e.g. opening a container) failed.
    Azalea(String),
    /// A container-open request never resolved within the timeout.
    ///
    /// No timeout exists inside Azalea's own `open_container_at` for this case, so
    /// one is enforced here -- an unopenable container (out of range, obstructed,
    /// wrong block) should be a bounded, reported failure, not a silent forever
    /// hang, matching this project's general refusal to let any operation block
    /// without an observable bound.
    ContainerOpenTimeout,
}

impl fmt::Display for BotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpawnTimeout => write!(formatter, "bot did not reach Event::Spawn in time"),
            Self::SpawnChannelClosed => {
                write!(formatter, "bot connection thread exited before spawning")
            }
            Self::BotThreadPanicked => write!(formatter, "bot connection thread panicked"),
            Self::Azalea(message) => write!(formatter, "azalea operation failed: {message}"),
            Self::ContainerOpenTimeout => {
                write!(formatter, "opening the container did not resolve in time")
            }
            Self::ShutdownTimeout => {
                write!(
                    formatter,
                    "bot connection thread did not finish after exit()"
                )
            }
        }
    }
}

impl std::error::Error for BotError {}

#[derive(Clone, Default, Component)]
struct SpawnRelay {
    sender: Arc<Mutex<Option<oneshot::Sender<Client>>>>,
}

async fn relay_client_on_spawn(
    bot: Client,
    event: Event,
    state: SpawnRelay,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Event::Spawn = event
        && let Some(sender) = state.sender.lock().expect("spawn-relay mutex").take()
    {
        let _ = sender.send(bot);
    }
    Ok(())
}

/// A real Azalea bot connected to a [`TestServer`]'s pinned Minecraft instance.
///
/// `ClientBuilder::start`'s future is not `Send` (confirmed against the real
/// library, not assumed), so the connection runs on a dedicated OS thread with its
/// own single-threaded runtime and `LocalSet`, entirely separate from the runtime
/// used here to call ordinary [`Client`] methods once connected.
pub struct BotHandle {
    caller_runtime: tokio::runtime::Runtime,
    client: Client,
    bot_thread: Option<JoinHandle<()>>,
    finished_rx: mpsc::Receiver<()>,
}

impl BotHandle {
    /// Connects a bot with offline auth to the given server's reserved game port.
    ///
    /// # Errors
    ///
    /// Returns [`BotError::SpawnTimeout`] if the bot does not reach
    /// [`Event::Spawn`] within 30 seconds, or [`BotError::SpawnChannelClosed`] if
    /// the connection thread exits (panics) before spawning.
    pub fn connect(server: &TestServer, name: &str) -> Result<Self> {
        let port = server.game_port();
        let account_name = name.to_owned();
        let (spawn_tx, spawn_rx) = oneshot::channel();
        let state = SpawnRelay {
            sender: Arc::new(Mutex::new(Some(spawn_tx))),
        };

        let (finished_tx, finished_rx) = mpsc::channel();
        let bot_thread = thread::spawn(move || {
            let bot_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build bot-thread tokio runtime");
            let local_set = tokio::task::LocalSet::new();
            local_set.block_on(&bot_runtime, async move {
                let account = Account::offline(&account_name);
                ClientBuilder::new()
                    .set_handler(relay_client_on_spawn)
                    .set_state(state)
                    // A test bot must never silently retry a lost connection --
                    // that turns any real disconnect into a `shutdown()` that
                    // blocks forever waiting for a thread that will never return
                    // on its own (measured: this is a real hang, not a theoretical
                    // one). Any disconnect here should surface as an observable
                    // test failure instead.
                    .reconnect_after(None)
                    .start(account, format!("127.0.0.1:{port}"))
                    .await;
            });
            // `JoinHandle::join` has no timeout, so `stop` waits on this instead --
            // a `std::thread::JoinHandle` alone can't bound how long shutdown waits.
            let _ = finished_tx.send(());
        });

        let caller_runtime = tokio::runtime::Runtime::new().expect("build caller tokio runtime");
        let received = caller_runtime
            .block_on(async { tokio::time::timeout(DEFAULT_SPAWN_TIMEOUT, spawn_rx).await });

        let client = match received {
            Ok(Ok(client)) => client,
            Ok(Err(_)) => {
                let _ = bot_thread.join();
                return Err(BotError::SpawnChannelClosed);
            }
            Err(_) => return Err(BotError::SpawnTimeout),
        };

        Ok(Self {
            caller_runtime,
            client,
            bot_thread: Some(bot_thread),
            finished_rx,
        })
    }

    /// The underlying Azalea client, for operations this handle doesn't wrap.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Runs a future to completion on this bot's own runtime.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.caller_runtime.block_on(future)
    }

    /// Opens a container at the given block position and left-clicks one slot.
    ///
    /// Deliberately does not close the container afterward: measured directly
    /// against the pinned server, closing it immediately after the click races the
    /// server's own click-acknowledgement packet ("Got SetContainerContentEvent for
    /// container with ID 1, but the current container ID is 0"), which was the
    /// proximate cause of a real mid-test disconnect. The container is implicitly
    /// closed when this bot later disconnects.
    ///
    /// Returns `Ok(false)`, not an error, if no container could be opened at that
    /// position -- fail-soft, matching this project's established convention for
    /// "wrong block/no block there" rather than treating it as exceptional.
    ///
    /// # Errors
    ///
    /// Returns [`BotError::Azalea`] if the underlying container-open call fails, or
    /// [`BotError::ContainerOpenTimeout`] if it never resolves within 10 seconds.
    pub fn open_container_and_click(&self, pos: BlockPos, slot: usize) -> Result<bool> {
        self.block_on(async {
            let attempt = self.client.open_container_at(pos);
            let opened = tokio::time::timeout(DEFAULT_CONTAINER_OPEN_TIMEOUT, attempt)
                .await
                .map_err(|_| BotError::ContainerOpenTimeout)?
                .map_err(|error| BotError::Azalea(error.to_string()))?;
            let Some(container) = opened else {
                return Ok(false);
            };
            container.left_click(slot);
            Ok(true)
        })
    }

    /// Disconnects the bot and stops its background connection thread.
    ///
    /// # Errors
    ///
    /// Returns [`BotError::ShutdownTimeout`] if the connection thread does not
    /// finish within 15 seconds of `exit()` (left running, not joined, rather than
    /// blocking forever), or [`BotError::BotThreadPanicked`] if it panicked.
    pub fn shutdown(mut self) -> Result<()> {
        self.stop()
    }

    fn stop(&mut self) -> Result<()> {
        // `Client::exit` is a plain synchronous call
        // (`self.ecs.write().write_message(...)`) with no timeout of its own --
        // measured directly that it can block indefinitely if the bot thread's own
        // tick loop is holding that same ECS write lock (observed after the
        // container-content ID-mismatch warning `open_container_and_click`'s doc
        // comment already flags). Calling it on its own detached thread means a
        // stuck `exit()` blocks that thread, not this one, so the bounded wait
        // below still reliably fires either way.
        let client = self.client.clone();
        let _ = thread::spawn(move || client.exit());
        match self.finished_rx.recv_timeout(DEFAULT_SHUTDOWN_TIMEOUT) {
            Ok(()) => {
                if let Some(thread) = self.bot_thread.take() {
                    thread.join().map_err(|_| BotError::BotThreadPanicked)?;
                }
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => {
                self.bot_thread = None;
                Err(BotError::ShutdownTimeout)
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The sender only drops after sending, or if the thread panicked
                // first without sending.
                if let Some(thread) = self.bot_thread.take() {
                    thread.join().map_err(|_| BotError::BotThreadPanicked)?;
                }
                Ok(())
            }
        }
    }
}

impl Drop for BotHandle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
