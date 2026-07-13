use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    for variable in ["RUSTC", "PROFILE", "TARGET"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }

    let rustc = required_variable("RUSTC");
    let rustc_verbose = rustc_verbose(&rustc);
    let cargo_profile = required_variable("PROFILE");
    let build_target = required_variable("TARGET");
    let out_dir = PathBuf::from(required_variable("OUT_DIR"));
    let generated = format!(
        "pub const RUSTC_VERBOSE: &str = {rustc_verbose:?};\n\
         pub const CARGO_PROFILE: &str = {cargo_profile:?};\n\
         pub const BUILD_TARGET: &str = {build_target:?};\n"
    );
    fs::write(out_dir.join("measurement_build_metadata.rs"), generated)
        .expect("write generated measurement build metadata");
}

fn required_variable(name: &str) -> String {
    env::var(name).unwrap_or_else(|error| panic!("Cargo did not provide {name}: {error}"))
}

fn rustc_verbose(rustc: &str) -> String {
    let output = Command::new(rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|error| panic!("run Cargo's selected rustc {rustc:?}: {error}"));
    assert!(
        output.status.success(),
        "Cargo's selected rustc {rustc:?} rejected -vV with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let verbose = String::from_utf8(output.stdout)
        .expect("Cargo's selected rustc emitted non-UTF-8 version metadata");
    let verbose = verbose.trim();
    assert!(
        !verbose.is_empty(),
        "Cargo's selected rustc emitted empty version metadata"
    );
    verbose.to_owned()
}
