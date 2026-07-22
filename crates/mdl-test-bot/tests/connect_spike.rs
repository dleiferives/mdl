use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use azalea::prelude::*;
use mdl_test::{ServerConfig, ServerSandbox};
use tokio::sync::oneshot;

/// PS-13 Stage 2: the riskiest new mechanism in this milestone, hand-verified in
/// isolation before any `BotHandle` abstraction is built on top of it -- same
/// discipline BE-1 used for its own riskiest assumptions. Proves, against the real
/// pinned server: a real Azalea bot can connect with offline auth to the port
/// reserved by `ServerSandbox`, reach `Event::Spawn`, and be cleanly stopped with
/// `Client::exit()` (not `disconnect()` -- confirmed by reading Azalea's own source
/// that only `exit()` is documented to return control from `ClientBuilder::start`).
///
/// `ClientBuilder::start(...)`'s returned future is **not `Send`** (it internally
/// runs on a `tokio::task::LocalSet` -- confirmed by trying `tokio::spawn` on it
/// first and reading the resulting `*const () cannot be shared between threads`
/// error). It cannot be driven from an ordinary multi-threaded-runtime `spawn`. It
/// has to run on a dedicated OS thread with its own single-threaded runtime and
/// `LocalSet`, entirely separate from the runtime used to call ordinary `Client`
/// methods afterward.
#[derive(Clone, Default, Component)]
struct SpawnRelay {
    sender: Arc<Mutex<Option<oneshot::Sender<Client>>>>,
}

async fn relay_client_on_spawn(
    bot: Client,
    event: Event,
    state: SpawnRelay,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Event::Spawn = event
        && let Some(sender) = state.sender.lock().expect("spawn-relay mutex").take()
    {
        let _ = sender.send(bot);
    }
    Ok(())
}

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and nightly Rust"]
fn bot_can_connect_reach_spawn_and_exit_cleanly() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let config = ServerConfig::new(java, server_jar);

    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    let port = sandbox.game_port();
    let server = sandbox.start(&config).expect("start server");

    let (spawn_tx, spawn_rx) = oneshot::channel();
    let state = SpawnRelay {
        sender: Arc::new(Mutex::new(Some(spawn_tx))),
    };

    let bot_thread = thread::spawn(move || {
        let bot_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build bot-thread tokio runtime");
        let local_set = tokio::task::LocalSet::new();
        local_set.block_on(&bot_runtime, async move {
            let account = Account::offline("mdl-test-bot");
            ClientBuilder::new()
                .set_handler(relay_client_on_spawn)
                .set_state(state)
                .start(account, format!("127.0.0.1:{port}"))
                .await;
        });
    });

    let caller_runtime = tokio::runtime::Runtime::new().expect("build caller tokio runtime");
    let client = caller_runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(20), spawn_rx)
            .await
            .expect("bot should reach Event::Spawn before timing out")
            .expect("spawn-relay sender must not be dropped without sending")
    });

    client.exit();
    bot_thread
        .join()
        .expect("bot thread should join after exit");

    server.shutdown().expect("shutdown server");
}
