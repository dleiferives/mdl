use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_NONCE: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        for _ in 0..100 {
            let nonce = TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("mdl-cli-{label}-{}-{nonce}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("failed to create {}: {error}", path.display()),
            }
        }
        panic!("could not allocate a unique CLI test directory")
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

fn command(output: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mdl"));
    command.args([
        "compile",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scalar.mdl"),
        "--output",
    ]);
    command.arg(output);
    command.args([
        "--core-opt",
        "baseline",
        "--minecraft-opt",
        "baseline",
        "--namespace",
        "mdl_cli_test",
        "--register-objective",
        "mdl.cli",
        "--description",
        "MDL CLI integration test",
    ]);
    command
}

#[test]
fn checked_in_source_fixture_materializes_exact_artifact_and_reports_abi() {
    let temp = TempDirectory::new("success");
    let output_root = temp.path().join("pack");
    let output = command(&output_root).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("compiled "));
    assert!(stdout.contains("target analysis: complete\n"));
    assert!(stdout.contains("source function ABI:\n"));
    assert!(stdout.contains("function[0] choose entry mdl_cli_test:"));
    assert!(stdout.contains("parameter[0] bool -> "));
    assert!(stdout.contains("parameter[1] i32 -> "));
    assert!(stdout.contains("result[0] i32 -> "));
    assert!(stdout.contains("function[1] is_zero entry mdl_cli_test:"));

    let mut files = walk_files(&output_root);
    files.sort();
    assert_eq!(files, expected_files());
    assert_eq!(
        fs::read_to_string(output_root.join("pack.mcmeta")).unwrap(),
        "{\"pack\":{\"description\":\"MDL CLI integration test\",\"min_format\":[107,1],\"max_format\":[107,1]}}\n"
    );
}

#[test]
fn existing_empty_output_root_is_replaced_without_staging_residue() {
    let temp = TempDirectory::new("existing-empty");
    let output_root = temp.path().join("pack");
    fs::create_dir(&output_root).unwrap();

    let output = command(&output_root).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut files = walk_files(&output_root);
    files.sort();
    assert_eq!(files, expected_files());
    let root_entries = fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(root_entries, ["pack"]);
}

#[test]
fn source_diagnostics_are_rendered_and_do_not_create_output() {
    let temp = TempDirectory::new("diagnostic");
    let source = temp.path().join("invalid.mdl");
    let output_root = temp.path().join("pack");
    fs::write(&source, "fn broken() -> Int32 { return nope; }\n").unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_mdl"));
    command
        .arg("compile")
        .arg(&source)
        .args(["--output"])
        .arg(&output_root)
        .args([
            "--core-opt",
            "none",
            "--minecraft-opt",
            "none",
            "--namespace",
            "mdl_cli_test",
            "--register-objective",
            "mdl.cli",
            "--description",
            "invalid",
        ]);
    let output = command.output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("error[frontend.check.unknown-name]"));
    assert!(stderr.contains("1 | fn broken() -> Int32 { return nope; }"));
    assert!(!output_root.exists());
}

#[test]
fn target_lowering_diagnostics_remain_source_located() {
    let temp = TempDirectory::new("lowering-diagnostic");
    let source = temp.path().join("recursive.mdl");
    let output_root = temp.path().join("pack");
    fs::write(
        &source,
        "fn again(flag: Bool) -> Bool { if (flag) { return again(false); } return flag; }\n",
    )
    .unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_mdl"));
    command
        .arg("compile")
        .arg(&source)
        .args(["--output"])
        .arg(&output_root)
        .args([
            "--core-opt",
            "none",
            "--minecraft-opt",
            "none",
            "--namespace",
            "mdl_cli_test",
            "--register-objective",
            "mdl.cli",
            "--description",
            "recursive",
        ]);
    let output = command.output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("error[lower.recursive-call-abi]"));
    assert!(stderr.contains("1 | fn again(flag: Bool) -> Bool"));
    assert!(!output_root.exists());
}

#[test]
fn nonempty_output_refusal_is_atomic() {
    let temp = TempDirectory::new("refusal");
    let output_root = temp.path().join("pack");
    fs::create_dir(&output_root).unwrap();
    let marker = output_root.join("owned.txt");
    fs::write(&marker, b"do not replace").unwrap();

    let output = command(&output_root).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing nonempty output root"));
    assert_eq!(fs::read(&marker).unwrap(), b"do not replace");
    assert_eq!(fs::read_dir(&output_root).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_output_root_is_refused_without_following_it() {
    use std::os::unix::fs::symlink;

    let temp = TempDirectory::new("symlink");
    let destination = temp.path().join("destination");
    let output_root = temp.path().join("pack");
    fs::create_dir(&destination).unwrap();
    symlink(&destination, &output_root).unwrap();

    let output = command(&output_root).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing symlink output root"));
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    assert!(
        fs::symlink_metadata(&output_root)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = vec![];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else {
                files.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    files
}

fn expected_files() -> [PathBuf; 9] {
    [
        "data/mdl_cli_test/function/__mdl/f0/b0.mcfunction",
        "data/mdl_cli_test/function/__mdl/f0/b1.mcfunction",
        "data/mdl_cli_test/function/__mdl/f0/b2.mcfunction",
        "data/mdl_cli_test/function/__mdl/f0/b3.mcfunction",
        "data/mdl_cli_test/function/__mdl/f1/b0.mcfunction",
        "data/mdl_cli_test/function/__mdl/init/try_create.mcfunction",
        "data/mdl_cli_test/function/__mdl/load.mcfunction",
        "data/minecraft/tags/function/load.json",
        "pack.mcmeta",
    ]
    .map(PathBuf::from)
}
