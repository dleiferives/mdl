use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use mdl_test::{ServerConfig, run_smoke_test};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        return Err(usage());
    };
    if matches!(command.to_str(), Some("--help" | "-h")) {
        println!("{}", usage());
        return Ok(());
    }
    if command != "smoke" {
        return Err(format!("unknown command {command:?}\n\n{}", usage()));
    }

    let mut server_jar = env::var_os("MDL_SERVER_JAR").map(PathBuf::from);
    let mut java = env::var_os("MDL_JAVA").map_or_else(default_java, PathBuf::from);
    let mut keep = env_flag("MDL_KEEP_TEST_DIR");

    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--server-jar") => {
                server_jar = Some(PathBuf::from(next_value(&mut args, "--server-jar")?));
            }
            Some("--java") => java = PathBuf::from(next_value(&mut args, "--java")?),
            Some("--keep") => keep = true,
            Some("--help" | "-h") => {
                println!("{}", usage());
                return Ok(());
            }
            _ => return Err(format!("unknown argument {argument:?}\n\n{}", usage())),
        }
    }

    let server_jar = server_jar.ok_or_else(|| {
        format!(
            "missing Minecraft server JAR; pass --server-jar or set MDL_SERVER_JAR\n\n{}",
            usage()
        )
    })?;
    let config = ServerConfig::new(java, server_jar);
    let preserved_root = run_smoke_test(&config, keep).map_err(|error| error.to_string())?;

    println!("vanilla smoke test passed");
    if let Some(root) = preserved_root {
        println!("server sandbox preserved at {}", root.display());
    }
    Ok(())
}

fn next_value(args: &mut impl Iterator<Item = OsString>, option: &str) -> Result<OsString, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn default_java() -> PathBuf {
    let homebrew_java = PathBuf::from("/opt/homebrew/opt/openjdk@25/bin/java");
    if homebrew_java.is_file() {
        homebrew_java
    } else {
        PathBuf::from("java")
    }
}

fn env_flag(name: &str) -> bool {
    env::var_os(name)
        .is_some_and(|value| !matches!(value.to_str(), Some("" | "0" | "false" | "no")))
}

fn usage() -> String {
    "Usage: mdl-test smoke [--server-jar PATH] [--java PATH] [--keep]\n\
     \n\
     Environment: MDL_SERVER_JAR, MDL_JAVA, MDL_KEEP_TEST_DIR"
        .to_owned()
}
