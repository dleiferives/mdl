use std::env;
use std::path::PathBuf;

use mdl_test::{ServerConfig, run_smoke_test};

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR"]
fn generated_pack_runs_on_vanilla() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let config = ServerConfig::new(java, server_jar);

    run_smoke_test(&config, preserve).expect("vanilla smoke test should pass");
}
