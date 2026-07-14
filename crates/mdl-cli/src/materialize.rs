//! Transactional filesystem materialization of an in-memory datapack.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use mdl_compiler::datapack::DatapackArtifact;

static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);
const MAX_STAGING_ATTEMPTS: usize = 100;

/// Materializes a complete artifact through a same-parent staging directory.
///
/// The artifact is fully preflighted before touching the filesystem. An existing
/// output root must be a real, empty directory; symlinks, non-directories, and
/// nonempty directories are rejected. Files are written in logical path order to a
/// freshly and exclusively created sibling, then renamed into place.
///
/// # Errors
///
/// Returns a typed preflight or filesystem error. A rejected preflight never writes
/// into the output root. Filesystem failures can leave newly created empty parent
/// directories, or a hidden sibling staging directory if the operating system also
/// refuses best-effort cleanup.
pub fn materialize_artifact(
    artifact: &DatapackArtifact,
    output_root: &Path,
) -> Result<(), MaterializeError> {
    let files = preflight_artifact(artifact)?;
    let root_state = preflight_output_root(output_root)?;
    let parent = output_root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root_name = output_root
        .file_name()
        .ok_or_else(|| MaterializeError::InvalidOutputRoot {
            path: output_root.to_path_buf(),
        })?;

    fs::create_dir_all(parent)
        .map_err(|source| MaterializeError::io("create output parent", parent, source))?;
    let staging = create_staging_directory(parent, root_name)?;

    let write_result = write_files(&staging, &files);
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if root_state == OutputRootState::ExistingEmpty {
        if let Err(error) = verify_existing_root_still_empty(output_root) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        fs::remove_dir(output_root).map_err(|source| {
            let _ = fs::remove_dir_all(&staging);
            MaterializeError::io("remove empty output root", output_root, source)
        })?;
    }

    if let Err(source) = fs::rename(&staging, output_root) {
        if root_state == OutputRootState::ExistingEmpty {
            let _ = fs::create_dir(output_root);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(MaterializeError::io(
            "install staged artifact",
            output_root,
            source,
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputRootState {
    Absent,
    ExistingEmpty,
}

struct PreparedFile<'a> {
    relative: PathBuf,
    bytes: &'a [u8],
}

fn preflight_artifact(
    artifact: &DatapackArtifact,
) -> Result<Vec<PreparedFile<'_>>, MaterializeError> {
    let mut files = Vec::with_capacity(artifact.files().len());
    let mut file_paths = BTreeSet::new();
    let mut directory_paths = BTreeSet::new();

    for file in artifact.files() {
        let logical = file.path().as_str();
        let relative = checked_relative_path(logical)?;
        if !file_paths.insert(relative.clone()) {
            return Err(MaterializeError::ArtifactPathCollision {
                path: logical.to_owned(),
            });
        }
        if directory_paths.contains(&relative) {
            return Err(MaterializeError::ArtifactPathCollision {
                path: logical.to_owned(),
            });
        }

        let mut parent = relative.parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            if file_paths.contains(path) {
                return Err(MaterializeError::ArtifactPathCollision {
                    path: logical.to_owned(),
                });
            }
            directory_paths.insert(path.to_path_buf());
            parent = path.parent();
        }
        files.push(PreparedFile {
            relative,
            bytes: file.bytes(),
        });
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn checked_relative_path(logical: &str) -> Result<PathBuf, MaterializeError> {
    let path = Path::new(logical);
    if logical.is_empty()
        || logical.starts_with('/')
        || logical.contains('\\')
        || logical
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || path.is_absolute()
    {
        return Err(MaterializeError::UnsafeArtifactPath {
            path: logical.to_owned(),
        });
    }
    let mut checked = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => checked.push(segment),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => {
                return Err(MaterializeError::UnsafeArtifactPath {
                    path: logical.to_owned(),
                });
            }
        }
    }
    if checked.as_os_str().is_empty() {
        return Err(MaterializeError::UnsafeArtifactPath {
            path: logical.to_owned(),
        });
    }
    Ok(checked)
}

fn preflight_output_root(output_root: &Path) -> Result<OutputRootState, MaterializeError> {
    if output_root.as_os_str().is_empty() || output_root.file_name().is_none() {
        return Err(MaterializeError::InvalidOutputRoot {
            path: output_root.to_path_buf(),
        });
    }
    match fs::symlink_metadata(output_root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(MaterializeError::OutputRootSymlink {
                path: output_root.to_path_buf(),
            })
        }
        Ok(metadata) if !metadata.is_dir() => Err(MaterializeError::OutputRootNotDirectory {
            path: output_root.to_path_buf(),
        }),
        Ok(_) => {
            let mut entries = fs::read_dir(output_root).map_err(|source| {
                MaterializeError::io("inspect output root", output_root, source)
            })?;
            if entries
                .next()
                .transpose()
                .map_err(|source| MaterializeError::io("inspect output root", output_root, source))?
                .is_some()
            {
                Err(MaterializeError::OutputRootNotEmpty {
                    path: output_root.to_path_buf(),
                })
            } else {
                Ok(OutputRootState::ExistingEmpty)
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(OutputRootState::Absent),
        Err(source) => Err(MaterializeError::io(
            "inspect output root",
            output_root,
            source,
        )),
    }
}

fn verify_existing_root_still_empty(output_root: &Path) -> Result<(), MaterializeError> {
    match preflight_output_root(output_root)? {
        OutputRootState::ExistingEmpty => Ok(()),
        OutputRootState::Absent => Err(MaterializeError::OutputRootChanged {
            path: output_root.to_path_buf(),
        }),
    }
}

fn create_staging_directory(
    parent: &Path,
    root_name: &std::ffi::OsStr,
) -> Result<PathBuf, MaterializeError> {
    let root_name = root_name.to_string_lossy();
    for _ in 0..MAX_STAGING_ATTEMPTS {
        let nonce = STAGING_NONCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".{root_name}.mdl-stage-{}-{nonce}", process::id());
        let staging = parent.join(name);
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(MaterializeError::io(
                    "create staging directory",
                    &staging,
                    source,
                ));
            }
        }
    }
    Err(MaterializeError::StagingNameExhausted {
        parent: parent.to_path_buf(),
    })
}

fn write_files(staging: &Path, files: &[PreparedFile<'_>]) -> Result<(), MaterializeError> {
    for prepared in files {
        let destination = staging.join(&prepared.relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                MaterializeError::io("create artifact directory", parent, source)
            })?;
        }
        let mut file = create_new_file(&destination)?;
        file.write_all(prepared.bytes)
            .map_err(|source| MaterializeError::io("write artifact file", &destination, source))?;
    }
    Ok(())
}

fn create_new_file(path: &Path) -> Result<File, MaterializeError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| MaterializeError::io("create artifact file", path, source))
}

/// Failure to preflight or materialize an in-memory datapack artifact.
#[derive(Debug)]
#[non_exhaustive]
pub enum MaterializeError {
    /// The user-selected root cannot name a sibling staging directory.
    InvalidOutputRoot { path: PathBuf },
    /// The output root is a symlink and could redirect writes.
    OutputRootSymlink { path: PathBuf },
    /// The output root exists but is not a directory.
    OutputRootNotDirectory { path: PathBuf },
    /// The output root already contains user data.
    OutputRootNotEmpty { path: PathBuf },
    /// The output root changed after preflight and before installation.
    OutputRootChanged { path: PathBuf },
    /// A supposedly validated logical artifact path was not safely relative.
    UnsafeArtifactPath { path: String },
    /// Artifact files collide as a file/directory pair or exact path.
    ArtifactPathCollision { path: String },
    /// Every bounded staging-directory candidate already existed.
    StagingNameExhausted { parent: PathBuf },
    /// A concrete filesystem operation failed.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl MaterializeError {
    fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl fmt::Display for MaterializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOutputRoot { path } => {
                write!(formatter, "invalid output root {}", path.display())
            }
            Self::OutputRootSymlink { path } => {
                write!(formatter, "refusing symlink output root {}", path.display())
            }
            Self::OutputRootNotDirectory { path } => write!(
                formatter,
                "output root {} is not a directory",
                path.display()
            ),
            Self::OutputRootNotEmpty { path } => write!(
                formatter,
                "refusing nonempty output root {}",
                path.display()
            ),
            Self::OutputRootChanged { path } => write!(
                formatter,
                "output root {} changed while installing the artifact",
                path.display()
            ),
            Self::UnsafeArtifactPath { path } => {
                write!(formatter, "unsafe relative artifact path `{path}`")
            }
            Self::ArtifactPathCollision { path } => {
                write!(formatter, "colliding artifact path `{path}`")
            }
            Self::StagingNameExhausted { parent } => write!(
                formatter,
                "could not reserve a staging directory below {}",
                parent.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} at {}: {source}",
                path.display()
            ),
        }
    }
}

impl Error for MaterializeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidOutputRoot { .. }
            | Self::OutputRootSymlink { .. }
            | Self::OutputRootNotDirectory { .. }
            | Self::OutputRootNotEmpty { .. }
            | Self::OutputRootChanged { .. }
            | Self::UnsafeArtifactPath { .. }
            | Self::ArtifactPathCollision { .. }
            | Self::StagingNameExhausted { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{MaterializeError, checked_relative_path};

    #[test]
    fn artifact_paths_must_be_normalized_and_strictly_relative() {
        assert_eq!(
            checked_relative_path("data/mdl/function/main.mcfunction").unwrap(),
            PathBuf::from("data/mdl/function/main.mcfunction")
        );
        for invalid in [
            "",
            "/pack.mcmeta",
            "../outside",
            "data/./function",
            "data//function",
            "data\\outside",
        ] {
            assert!(matches!(
                checked_relative_path(invalid),
                Err(MaterializeError::UnsafeArtifactPath { .. })
            ));
        }
        assert!(!Path::new("data/mdl/function/main.mcfunction").is_absolute());
    }
}
