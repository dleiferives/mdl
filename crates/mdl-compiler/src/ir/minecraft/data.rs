use super::{FiniteF64, NbtValue, StoragePath};

/// A native `/data modify` operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DataModifyMode {
    /// Replace the target with the source.
    Set,
    /// Merge a compound source into the target.
    Merge,
    /// Append the source to a target list.
    Append,
    /// Prepend the source to a target list.
    Prepend,
}

/// A statically represented `/data modify` source.
#[derive(Clone, Debug, PartialEq)]
pub enum DataSource {
    /// One immutable literal NBT value.
    Value(NbtValue),
    /// One static path in command storage.
    From(StoragePath),
}

/// The closed initial storage-data command vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum DataCommand {
    /// Read one static storage path with an optional numeric scale.
    Get {
        /// Value source.
        source: StoragePath,
        /// Optional result multiplier.
        scale: Option<FiniteF64>,
    },
    /// Remove every value selected by one static storage path.
    Remove {
        /// Removal target.
        target: StoragePath,
    },
    /// Modify one static storage path from a literal or another static path.
    Modify {
        /// Modification target.
        target: StoragePath,
        /// Native modification mode.
        mode: DataModifyMode,
        /// Literal or storage-path source.
        source: DataSource,
    },
}

#[cfg(test)]
mod tests {
    use super::{DataCommand, DataModifyMode, DataSource};
    use crate::ir::minecraft::{
        FiniteF64, NbtPath, NbtPathKey, NbtPathSegment, NbtValue, StorageId, StoragePath,
    };

    fn path(storage: &str, key: &str) -> StoragePath {
        StoragePath::new(
            StorageId::parse(storage).unwrap(),
            NbtPath::new(NbtPathSegment::Key(NbtPathKey::new(key).unwrap()), vec![]),
        )
    }

    #[test]
    fn storage_commands_retain_static_paths_modes_and_sources() {
        let get = DataCommand::Get {
            source: path("mdl:state", "count"),
            scale: Some(FiniteF64::new(2.0).unwrap()),
        };
        assert!(matches!(get, DataCommand::Get { scale: Some(_), .. }));

        let modify = DataCommand::Modify {
            target: path("mdl:state", "items"),
            mode: DataModifyMode::Append,
            source: DataSource::Value(NbtValue::int(7)),
        };
        assert!(matches!(
            modify,
            DataCommand::Modify {
                mode: DataModifyMode::Append,
                source: DataSource::Value(_),
                ..
            }
        ));

        let from = DataSource::From(path("mdl:other", "source"));
        assert!(matches!(from, DataSource::From(_)));
    }
}
