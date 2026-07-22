use std::env;
use std::path::PathBuf;

use mdl_test::{ServerConfig, ServerSandbox};
use mdl_test_bot::BotHandle;

/// PS-13 Stage 3: `BotHandle::connect`/`shutdown` against the real pinned server --
/// the same round trip Stage 2's throwaway spike hand-verified, now through the real
/// abstraction other PS milestones will build on.
#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and nightly Rust"]
fn bot_handle_connects_and_shuts_down_cleanly() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let config = ServerConfig::new(java, server_jar);

    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    let server = sandbox.start(&config).expect("start server");

    let bot = BotHandle::connect(&server, "mdl-test-bot").expect("bot should connect and spawn");
    bot.shutdown().expect("bot should shut down cleanly");

    server.shutdown().expect("shutdown server");
}
