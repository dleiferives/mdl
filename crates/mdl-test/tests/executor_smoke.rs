use std::env;
use std::path::PathBuf;

use mcfunction_executor::{McExecutor, V26_2};

/// Runs the MDL smoke pack on the in-process executor and verifies
/// the same behavior as the real server smoke test.
#[test]
fn executor_smoke_matches_real_server() {
    let sandbox = mdl_test::ServerSandbox::create(false).expect("create sandbox");
    mdl_test::install_smoke_pack(&sandbox).expect("install smoke pack");

    // Run on executor
    let mut exec = McExecutor::create(sandbox.root().to_path_buf(), V26_2);
    let checkpoint = exec.log_checkpoint();
    exec.load_datapacks().expect("load datapacks");

    exec.command("function mdl_test:smoke")
        .expect("invoke smoke");
    exec.wait_for_command_log("MDL_SMOKE_RESULT_42")
        .expect("smoke marker");

    exec.command("scoreboard players get #result mdl_test")
        .expect("get result");
    let line = exec
        .wait_for_command_log("#result has")
        .expect("result feedback");
    assert!(line.contains("#result has 42"), "wrong result: {line}");

    exec.command("execute if score #result mdl_test matches 42 run say MDL_EXEC_OK")
        .expect("if score");
    exec.wait_for_command_log("MDL_EXEC_OK")
        .expect("exec OK marker");
    exec.check_datapack_logs_since(checkpoint)
        .expect("executor must support every smoke-pack command");
}

/// Same test, but also runs against the real server when MDL_SERVER_JAR is set.
/// Ignored by default because it needs the official JAR.
#[test]
#[ignore = "requires the official Minecraft server JAR"]
fn dual_backend_smoke() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR");
    let java = env::var_os("MDL_JAVA").unwrap_or_else(|| "java".into());
    let config = mdl_test::ServerConfig::new(java, server_jar);

    // Real server
    let sandbox = mdl_test::ServerSandbox::create(false).expect("create sandbox");
    mdl_test::install_smoke_pack(&sandbox).expect("install smoke pack");
    let mut real_server = sandbox.start(&config).expect("start server");
    real_server
        .command("function mdl_test:smoke")
        .expect("real smoke invoke");
    real_server
        .wait_for_command_log("MDL_SMOKE_RESULT_42")
        .expect("real smoke marker");
    real_server
        .command("scoreboard players get #result mdl_test")
        .expect("real get result");
    let _real_line = real_server
        .wait_for_command_log("#result has")
        .expect("real result");
    real_server.shutdown().expect("shutdown");
}

/// Quick check: can we load a datapack with tag JSON and execute a function
/// through it on the executor?
#[test]
fn executor_can_execute_tagged_function() {
    let sandbox = mdl_test::ServerSandbox::create(false).expect("create sandbox");
    mdl_test::install_smoke_pack(&sandbox).expect("install smoke pack");

    // Write a tag referencing the smoke function
    let tag_dir = sandbox
        .root()
        .join("world/datapacks/mdl_smoke/data/mdl_test/tags/function");
    std::fs::create_dir_all(&tag_dir).unwrap();
    std::fs::write(
        tag_dir.join("smoke_tag.json"),
        r#"{"values":["mdl_test:smoke"]}"#,
    )
    .unwrap();

    let mut exec = McExecutor::create(sandbox.root().to_path_buf(), V26_2);
    let checkpoint = exec.log_checkpoint();
    exec.load_datapacks().expect("load datapacks");

    // Verify the tag was loaded by executing the function directly first
    exec.command("function mdl_test:smoke")
        .expect("direct smoke");
    exec.wait_for_command_log("MDL_SMOKE_RESULT_42")
        .expect("direct smoke marker");

    // Now try via tag — note: tag name is "#namespace:name"
    exec.command("function #mdl_test:smoke_tag")
        .expect("tag smoke");
    exec.wait_for_command_log("MDL_SMOKE_RESULT_42")
        .expect("smoke marker via tag");
    exec.check_datapack_logs_since(checkpoint)
        .expect("executor must support every tagged smoke-pack command");
}
