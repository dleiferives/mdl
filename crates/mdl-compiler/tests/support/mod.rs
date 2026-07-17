use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const BLESS_ENVIRONMENT: &str = "MDL_BLESS";

pub(crate) fn assert_golden(relative_path: &str, embedded_expected: &str, actual: &str) {
    if embedded_expected == actual {
        return;
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let expected_path = manifest.join("tests").join(relative_path);
    if env::var_os(BLESS_ENVIRONMENT).is_some_and(|value| value == "1") {
        fs::write(&expected_path, actual)
            .unwrap_or_else(|error| panic!("cannot bless {}: {error}", expected_path.display()));
        eprintln!("blessed {}", expected_path.display());
        return;
    }

    let actual_path = failure_path(manifest, relative_path);
    if let Some(parent) = actual_path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", parent.display()));
    }
    fs::write(&actual_path, actual)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", actual_path.display()));
    panic!(
        "golden output changed: {}\nactual output: {}\ninspect with: diff -u {} {}\naccept only an intentional change with: {}=1 cargo test -p mdl-compiler --test stage4_lowering",
        expected_path.display(),
        actual_path.display(),
        expected_path.display(),
        actual_path.display(),
        BLESS_ENVIRONMENT
    );
}

fn failure_path(manifest: &Path, relative_path: &str) -> PathBuf {
    manifest
        .join("../../target/mdl-golden-failures")
        .join(relative_path)
        .with_extension("actual")
}
