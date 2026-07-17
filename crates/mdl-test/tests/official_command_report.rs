use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use mdl_test::{ServerSandbox, sha256_file};
use serde_json::Value;

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";

#[test]
#[ignore = "requires the official bundled Minecraft 26.2 server JAR and Java 25"]
fn java_26_2_command_report_matches_stage7_5_recipes() {
    if let Err(error) = check_command_report() {
        panic!("{error}");
    }
}

fn check_command_report() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "set MDL_SERVER_JAR to the official bundled Minecraft 26.2 server JAR".to_owned()
        })?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let actual_hash = sha256_file(&server_jar)
        .map_err(|error| format!("hash {}: {error}", server_jar.display()))?;
    if actual_hash != JAVA_26_2_SERVER_SHA256 {
        return Err(format!(
            "expected the pinned Java 26.2 server bundle SHA-256 {JAVA_26_2_SERVER_SHA256}, got {actual_hash}"
        ));
    }

    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let mut sandbox = ServerSandbox::create(preserve).map_err(|error| error.to_string())?;
    let output = Command::new(java)
        .current_dir(sandbox.root())
        .arg("-DbundlerMainClass=net.minecraft.data.Main")
        .arg("-jar")
        .arg(&server_jar)
        .arg("--reports")
        .output()
        .map_err(|error| format!("run the Java 26.2 data generator: {error}"))?;
    if !output.status.success() {
        sandbox.set_preserve(true);
        return Err(format!(
            "Java 26.2 data generator failed with {}; sandbox preserved at {}\n{}",
            output.status,
            sandbox.root().display(),
            bounded_process_output(&output.stdout, &output.stderr)
        ));
    }

    if let Err(error) = validate_report(sandbox.root()) {
        sandbox.set_preserve(true);
        return Err(format!(
            "{error}; generated report sandbox preserved at {}",
            sandbox.root().display()
        ));
    }
    Ok(())
}

fn validate_report(root: &Path) -> Result<(), String> {
    let report_path = root.join("generated/reports/commands.json");
    let report = read_json(&report_path)?;
    let say = report
        .pointer("/children/say")
        .ok_or_else(|| "commands.json has no root `say` command".to_owned())?;
    expect_json_string(say, "/type", "literal")?;
    let message = say
        .pointer("/children/message")
        .ok_or_else(|| "commands.json has no `say message` child".to_owned())?;
    expect_json_string(message, "/type", "argument")?;
    expect_json_string(message, "/parser", "minecraft:message")?;
    expect_executable(message, "say message")?;

    let execute = child(&report, "/children/execute", "execute")?;
    expect_parser(
        child(
            execute,
            "/children/align/children/axes",
            "execute align axes",
        )?,
        "minecraft:swizzle",
    )?;
    expect_parser(
        child(
            execute,
            "/children/anchored/children/anchor",
            "execute anchored anchor",
        )?,
        "minecraft:entity_anchor",
    )?;
    for branch in ["as", "at"] {
        let target = child(
            execute,
            &format!("/children/{branch}/children/targets"),
            &format!("execute {branch} targets"),
        )?;
        expect_parser(target, "minecraft:entity")?;
        expect_json_string(target, "/properties/amount", "multiple")?;
    }
    expect_parser(
        child(
            execute,
            "/children/positioned/children/pos",
            "execute positioned pos",
        )?,
        "minecraft:vec3",
    )?;
    expect_parser(
        child(
            execute,
            "/children/rotated/children/rot",
            "execute rotated rot",
        )?,
        "minecraft:rotation",
    )?;
    expect_parser(
        child(
            execute,
            "/children/in/children/dimension",
            "execute in dimension",
        )?,
        "minecraft:dimension",
    )?;

    let teleport = child(&report, "/children/teleport", "teleport")?;
    let location = child(teleport, "/children/location", "teleport location")?;
    expect_parser(location, "minecraft:vec3")?;
    expect_executable(location, "teleport location")?;
    let targets = child(teleport, "/children/targets", "teleport targets")?;
    expect_parser(targets, "minecraft:entity")?;
    expect_json_string(targets, "/properties/amount", "multiple")?;
    let target_location = child(targets, "/children/location", "teleport targets location")?;
    expect_parser(target_location, "minecraft:vec3")?;
    expect_executable(target_location, "teleport targets location")
}

fn child<'a>(value: &'a Value, pointer: &str, description: &str) -> Result<&'a Value, String> {
    value
        .pointer(pointer)
        .ok_or_else(|| format!("commands.json has no `{description}` node at {pointer}"))
}

fn expect_parser(value: &Value, expected: &str) -> Result<(), String> {
    expect_json_string(value, "/type", "argument")?;
    expect_json_string(value, "/parser", expected)
}

fn expect_executable(value: &Value, description: &str) -> Result<(), String> {
    if value.pointer("/executable").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(format!(
            "commands.json does not mark `{description}` executable"
        ))
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let input = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    serde_json::from_reader(input).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn expect_json_string(value: &Value, pointer: &str, expected: &str) -> Result<(), String> {
    let actual = value.pointer(pointer).and_then(Value::as_str);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "commands.json field {pointer} expected {expected:?}, got {actual:?}"
        ))
    }
}

fn bounded_process_output(stdout: &[u8], stderr: &[u8]) -> String {
    const LIMIT: usize = 8 * 1024;
    let mut output = Vec::with_capacity(stdout.len().saturating_add(stderr.len()).min(LIMIT));
    output.extend_from_slice(stdout);
    output.extend_from_slice(stderr);
    let start = output.len().saturating_sub(LIMIT);
    String::from_utf8_lossy(&output[start..]).into_owned()
}
