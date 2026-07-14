use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use mdl_test::{ServerConfig, ServerSandbox, TestServer};

static TEMP_NONCE: AtomicU64 = AtomicU64::new(0);

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn cli_materialized_pack_runs_on_vanilla_26_2() {
    if let Err(error) = run_server_proof() {
        panic!("{error}");
    }
}

fn run_server_proof() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let server_config = ServerConfig::new(java, server_jar);

    let temp = TempDirectory::new("server")?;
    let output_root = temp.path().join("cli-pack");
    let cli = spawn_cli(&output_root)?;
    let abi = parse_choose_abi(&cli)?;

    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    sandbox
        .install_datapack("mdl_stage6_cli", read_artifact(&output_root)?)
        .map_err(|error| error.to_string())?;

    let mut server = sandbox
        .start(&server_config)
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    let result = invoke_choose(&mut server, &abi);
    if let Err(error) = result {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 6 CLI server sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn spawn_cli(output_root: &Path) -> Result<String, String> {
    let output = Command::new(env!("CARGO_BIN_EXE_mdl"))
        .args([
            "compile",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scalar.mdl"),
            "--output",
        ])
        .arg(output_root)
        .args([
            "--core-opt",
            "baseline",
            "--minecraft-opt",
            "baseline",
            "--namespace",
            "mdl_stage6_cli",
            "--register-objective",
            "mdl6.cli",
            "--description",
            "MDL Stage 6 CLI server proof",
        ])
        .output()
        .map_err(|error| format!("failed to spawn mdl CLI: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "mdl CLI failed with {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("CLI stdout was not UTF-8: {error}"))
}

#[derive(Debug)]
struct PrintedAbi {
    entry: String,
    parameters: Vec<PrintedSlot>,
    result: PrintedSlot,
}

#[derive(Clone, Debug)]
struct PrintedSlot {
    holder: String,
    objective: String,
}

fn parse_choose_abi(output: &str) -> Result<PrintedAbi, String> {
    let lines = output.lines().collect::<Vec<_>>();
    let function = lines
        .iter()
        .position(|line| line.starts_with("  function[0] choose entry "))
        .ok_or_else(|| format!("CLI omitted the choose ABI:\n{output}"))?;
    let entry = lines[function]
        .strip_prefix("  function[0] choose entry ")
        .ok_or_else(|| "CLI choose entry prefix changed".to_owned())?
        .to_owned();
    let parameters = (0..3)
        .map(|index| {
            parse_slot(
                lines.get(function + index + 1).copied(),
                &format!("parameter[{index}]"),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = parse_slot(lines.get(function + 4).copied(), "result[0]")?;
    Ok(PrintedAbi {
        entry,
        parameters,
        result,
    })
}

fn parse_slot(line: Option<&str>, label: &str) -> Result<PrintedSlot, String> {
    let line = line.ok_or_else(|| format!("CLI omitted {label}"))?;
    let words = line.split_whitespace().collect::<Vec<_>>();
    if words.len() != 5 || words[0] != label || words[2] != "->" {
        return Err(format!("unexpected CLI {label} line: {line:?}"));
    }
    Ok(PrintedSlot {
        holder: words[3].to_owned(),
        objective: words[4].to_owned(),
    })
}

fn read_artifact(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = vec![];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf();
                let bytes = fs::read(&path)
                    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
                files.push((relative, bytes));
            } else {
                return Err(format!(
                    "CLI artifact contains a non-file entry: {}",
                    path.display()
                ));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn invoke_choose(server: &mut TestServer, abi: &PrintedAbi) -> Result<(), String> {
    for (slot, value) in abi.parameters.iter().zip([1, 42, -7]) {
        server
            .command(&format!(
                "scoreboard players set {} {} {value}",
                slot.holder, slot.objective
            ))
            .map_err(|error| error.to_string())?;
    }
    server
        .command(&format!("function {}", abi.entry))
        .map_err(|error| error.to_string())?;
    let marker = "MDL_STAGE6_CLI_RESULT_42";
    server
        .command(&format!(
            "execute if score {} {} matches 42 run say {marker}",
            abi.result.holder, abi.result.objective
        ))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map_err(|error| error.to_string())?;
    Ok(())
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Result<Self, String> {
        for _ in 0..100 {
            let nonce = TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("mdl-cli-{label}-{}-{nonce}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!("failed to create {}: {error}", path.display()));
                }
            }
        }
        Err("could not allocate CLI test directory".to_owned())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
