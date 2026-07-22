use std::env;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use mdl_test::{ServerConfig, ServerSandbox};

/// PS-13: a bot has to connect over a real TCP socket, unlike the console/stdin
/// driving this harness already does. `server-port=0` alone doesn't give a client
/// anywhere to connect to -- the server's own startup log echoes the *configured*
/// port back (`0`), not the OS-resolved one (measured directly against the pinned
/// server before this test was written, not assumed). This is the real claim PS-13's
/// bot infrastructure depends on: the port `ServerSandbox` reserves up front is the
/// exact port the running server ends up listening on, reachable by a plain TCP
/// client, not just "the server started successfully."
#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR"]
fn reserved_game_port_is_the_real_listening_port() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let config = ServerConfig::new(java, server_jar);

    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    let reserved_port = sandbox.game_port();
    assert_ne!(reserved_port, 0, "a concrete port must have been reserved");

    let server = sandbox.start(&config).expect("start server");
    assert_eq!(
        server.game_port(),
        reserved_port,
        "the running server must expose the same port that was reserved before startup"
    );

    let connected = TcpStream::connect_timeout(
        &format!("127.0.0.1:{reserved_port}").parse().unwrap(),
        Duration::from_secs(5),
    );
    assert!(
        connected.is_ok(),
        "the reserved port must be the real listening port a client can reach: {connected:?}"
    );

    server.shutdown().expect("shutdown server");
}
