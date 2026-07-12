use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::target::JavaEditionTarget;

/// The target-name domain in which validation failed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NameKind {
    /// A command-level resource namespace.
    Namespace,
    /// A command-level resource path.
    ResourcePath,
    /// A namespace that can become one artifact path component.
    PackNamespace,
    /// A resource path that can become normalized artifact path components.
    PackResourcePath,
    /// A function resource identifier.
    FunctionResource,
    /// A function-tag resource identifier.
    FunctionTagResource,
    /// A storage resource identifier.
    Storage,
    /// A dimension resource identifier.
    Dimension,
    /// A normalized logical datapack artifact path.
    PackPath,
}

impl fmt::Display for NameKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Namespace => "namespace",
            Self::ResourcePath => "resource path",
            Self::PackNamespace => "pack namespace",
            Self::PackResourcePath => "pack resource path",
            Self::FunctionResource => "function resource identifier",
            Self::FunctionTagResource => "function-tag resource identifier",
            Self::Storage => "storage identifier",
            Self::Dimension => "dimension identifier",
            Self::PackPath => "pack path",
        })
    }
}

/// The precise reason a target name was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NameErrorReason {
    /// The complete value was empty.
    Empty,
    /// A resource identifier omitted its explicit `:` separator.
    MissingNamespaceSeparator,
    /// A character is outside the target grammar.
    InvalidCharacter {
        /// Byte offset within the validated component.
        byte_index: usize,
        /// The rejected Unicode scalar value.
        character: char,
    },
    /// A logical path began at the host/filesystem root.
    AbsolutePath,
    /// A `/`-separated logical path contained an empty component.
    EmptySegment {
        /// Zero-based segment position.
        segment_index: usize,
    },
    /// A logical path contained the current-directory component `.`.
    CurrentDirectorySegment {
        /// Zero-based segment position.
        segment_index: usize,
    },
    /// A name contained the parent-directory component `..`.
    ParentDirectorySegment {
        /// Zero-based segment position.
        segment_index: usize,
    },
}

/// A failed target-name validation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NameError {
    kind: NameKind,
    reason: NameErrorReason,
}

impl NameError {
    const fn new(kind: NameKind, reason: NameErrorReason) -> Self {
        Self { kind, reason }
    }

    /// Returns the target-name domain that was being validated.
    #[must_use]
    pub const fn kind(self) -> NameKind {
        self.kind
    }

    /// Returns the precise rejection reason.
    #[must_use]
    pub const fn reason(self) -> NameErrorReason {
        self.reason
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid {}: ", self.kind)?;
        match self.reason {
            NameErrorReason::Empty => formatter.write_str("value cannot be empty"),
            NameErrorReason::MissingNamespaceSeparator => {
                formatter.write_str("an explicit `namespace:path` separator is required")
            }
            NameErrorReason::InvalidCharacter {
                byte_index,
                character,
            } => write!(
                formatter,
                "character {character:?} at byte {byte_index} is not accepted"
            ),
            NameErrorReason::AbsolutePath => formatter.write_str("path cannot be absolute"),
            NameErrorReason::EmptySegment { segment_index } => {
                write!(formatter, "segment {segment_index} cannot be empty")
            }
            NameErrorReason::CurrentDirectorySegment { segment_index } => write!(
                formatter,
                "segment {segment_index} cannot be the current directory `.`"
            ),
            NameErrorReason::ParentDirectorySegment { segment_index } => write!(
                formatter,
                "segment {segment_index} cannot be the parent directory `..`"
            ),
        }
    }
}

impl Error for NameError {}

/// An explicit, nonempty command-level resource namespace.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Namespace(Box<str>);

impl Namespace {
    /// Validates and owns a command-level namespace.
    ///
    /// # Errors
    ///
    /// Rejects empty values, the complete namespace `..`, and characters outside
    /// lowercase ASCII letters, digits, `_`, `-`, and `.`.
    pub fn new(value: &str) -> Result<Self, NameError> {
        validate_namespace(value)?;
        Ok(Self(value.into()))
    }

    /// Returns the validated namespace text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Namespace {
    type Err = NameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for Namespace {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_namespace(&value)?;
        Ok(Self(value.into_boxed_str()))
    }
}

/// An explicit, nonempty command-level resource path.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResourcePath(Box<str>);

impl ResourcePath {
    /// Validates and owns a command-level resource path.
    ///
    /// This type deliberately accepts `/`, empty path segments, and dot segments
    /// that vanilla accepts lexically. Use [`PackResourcePath`] before mapping a
    /// resource into an artifact.
    ///
    /// # Errors
    ///
    /// Rejects an empty value or a character outside the target resource-path
    /// alphabet.
    pub fn new(value: &str) -> Result<Self, NameError> {
        validate_resource_path(value)?;
        Ok(Self(value.into()))
    }

    /// Returns the validated resource path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourcePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ResourcePath {
    type Err = NameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for ResourcePath {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_resource_path(&value)?;
        Ok(Self(value.into_boxed_str()))
    }
}

/// A namespace proven safe as one logical datapack path component.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackNamespace(Namespace);

impl PackNamespace {
    /// Validates a namespace and its additional artifact-path invariant.
    ///
    /// # Errors
    ///
    /// Returns the command-level namespace error or rejects `.` as a path segment.
    pub fn new(value: &str) -> Result<Self, NameError> {
        Self::try_from(Namespace::new(value)?)
    }

    /// Returns the wrapped command-level namespace.
    #[must_use]
    pub const fn as_namespace(&self) -> &Namespace {
        &self.0
    }

    /// Returns the validated namespace text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<Namespace> for PackNamespace {
    type Error = NameError;

    fn try_from(namespace: Namespace) -> Result<Self, Self::Error> {
        match namespace.as_str() {
            "." => Err(NameError::new(
                NameKind::PackNamespace,
                NameErrorReason::CurrentDirectorySegment { segment_index: 0 },
            )),
            ".." => Err(NameError::new(
                NameKind::PackNamespace,
                NameErrorReason::ParentDirectorySegment { segment_index: 0 },
            )),
            _ => Ok(Self(namespace)),
        }
    }
}

impl From<PackNamespace> for Namespace {
    fn from(namespace: PackNamespace) -> Self {
        namespace.0
    }
}

impl fmt::Display for PackNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for PackNamespace {
    type Err = NameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for PackNamespace {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(Namespace::try_from(value)?)
    }
}

/// A resource path proven safe as normalized logical datapack path components.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackResourcePath(ResourcePath);

impl PackResourcePath {
    /// Validates a resource path and its additional artifact-path invariants.
    ///
    /// # Errors
    ///
    /// Returns the command-level resource-path error or rejects absolute paths,
    /// empty segments, and `.` / `..` segments.
    pub fn new(value: &str) -> Result<Self, NameError> {
        Self::try_from(ResourcePath::new(value)?)
    }

    /// Returns the wrapped command-level resource path.
    #[must_use]
    pub const fn as_resource_path(&self) -> &ResourcePath {
        &self.0
    }

    /// Returns the validated resource path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<ResourcePath> for PackResourcePath {
    type Error = NameError;

    fn try_from(path: ResourcePath) -> Result<Self, Self::Error> {
        validate_logical_segments(NameKind::PackResourcePath, path.as_str())?;
        Ok(Self(path))
    }
}

impl From<PackResourcePath> for ResourcePath {
    fn from(path: PackResourcePath) -> Self {
        path.0
    }
}

impl fmt::Display for PackResourcePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for PackResourcePath {
    type Err = NameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for PackResourcePath {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(ResourcePath::try_from(value)?)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ResourceLocation<N, P> {
    namespace: N,
    path: P,
}

impl<N, P> ResourceLocation<N, P> {
    const fn new(namespace: N, path: P) -> Self {
        Self { namespace, path }
    }

    const fn namespace(&self) -> &N {
        &self.namespace
    }

    const fn path(&self) -> &P {
        &self.path
    }
}

impl<N: fmt::Display, P: fmt::Display> fmt::Display for ResourceLocation<N, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.namespace, self.path)
    }
}

macro_rules! resource_id {
    (
        $(#[$attribute:meta])*
        $name:ident,
        $namespace:ty,
        $path:ty,
        $kind:expr
    ) => {
        $(#[$attribute])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(ResourceLocation<$namespace, $path>);

        impl $name {
            /// Constructs an identifier from validated components.
            #[must_use]
            pub const fn new(namespace: $namespace, path: $path) -> Self {
                Self(ResourceLocation::new(namespace, path))
            }

            /// Parses an explicit `namespace:path` identifier.
            ///
            /// # Errors
            ///
            /// Rejects a missing separator or either invalid component.
            pub fn parse(value: &str) -> Result<Self, NameError> {
                let (namespace, path) = split_resource_location($kind, value)?;
                Ok(Self::new(namespace.parse()?, path.parse()?))
            }

            /// Returns the validated namespace component.
            #[must_use]
            pub const fn namespace(&self) -> &$namespace {
                self.0.namespace()
            }

            /// Returns the validated path component.
            #[must_use]
            pub const fn path(&self) -> &$path {
                self.0.path()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = NameError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = NameError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }
    };
}

resource_id!(
    /// A function resource identifier that maps safely into a datapack.
    FunctionResourceId,
    PackNamespace,
    PackResourcePath,
    NameKind::FunctionResource
);

impl FunctionResourceId {
    /// Maps this resource into its target-specific function artifact path.
    #[must_use]
    pub fn pack_path(&self, target: JavaEditionTarget) -> PackPath {
        let spec = target.spec();
        PackPath::from_generated(format!(
            "data/{}/{}/{}.mcfunction",
            self.namespace().as_str(),
            spec.function_directory(),
            self.path().as_str()
        ))
    }
}

resource_id!(
    /// A function-tag resource identifier that maps safely into a datapack.
    FunctionTagResourceId,
    PackNamespace,
    PackResourcePath,
    NameKind::FunctionTagResource
);

impl FunctionTagResourceId {
    /// Maps this resource into its target-specific function-tag artifact path.
    #[must_use]
    pub fn pack_path(&self, target: JavaEditionTarget) -> PackPath {
        let spec = target.spec();
        PackPath::from_generated(format!(
            "data/{}/{}/{}.json",
            self.namespace().as_str(),
            spec.function_tag_directory(),
            self.path().as_str()
        ))
    }
}

resource_id!(
    /// A command-level storage identifier that never becomes an artifact path.
    StorageId,
    Namespace,
    ResourcePath,
    NameKind::Storage
);

resource_id!(
    /// A command-level dimension identifier that remains distinct from storage.
    DimensionId,
    Namespace,
    ResourcePath,
    NameKind::Dimension
);

/// A normalized logical path within an in-memory datapack artifact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackPath(Box<str>);

impl PackPath {
    /// Validates and owns a logical datapack artifact path.
    ///
    /// # Errors
    ///
    /// Rejects empty or absolute paths, characters outside the target resource-path
    /// alphabet, empty segments, and `.` / `..` segments.
    pub fn new(value: &str) -> Result<Self, NameError> {
        validate_pack_path(value)?;
        Ok(Self(value.into()))
    }

    /// Returns the root pack metadata path.
    #[must_use]
    pub fn metadata() -> Self {
        Self("pack.mcmeta".into())
    }

    fn from_generated(value: String) -> Self {
        Self::try_from(value).expect("closed target facts must generate a valid pack path")
    }

    /// Returns the normalized `/`-separated artifact path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PackPath {
    type Err = NameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for PackPath {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_pack_path(&value)?;
        Ok(Self(value.into_boxed_str()))
    }
}

fn validate_namespace(value: &str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::new(NameKind::Namespace, NameErrorReason::Empty));
    }
    validate_characters(NameKind::Namespace, value, is_namespace_character)?;
    if value == ".." {
        return Err(NameError::new(
            NameKind::Namespace,
            NameErrorReason::ParentDirectorySegment { segment_index: 0 },
        ));
    }
    Ok(())
}

fn validate_resource_path(value: &str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::new(
            NameKind::ResourcePath,
            NameErrorReason::Empty,
        ));
    }
    validate_characters(NameKind::ResourcePath, value, is_resource_path_character)
}

fn validate_pack_path(value: &str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::new(NameKind::PackPath, NameErrorReason::Empty));
    }
    validate_characters(NameKind::PackPath, value, is_resource_path_character)?;
    validate_logical_segments(NameKind::PackPath, value)
}

fn validate_characters(
    kind: NameKind,
    value: &str,
    accepts: fn(char) -> bool,
) -> Result<(), NameError> {
    if let Some((byte_index, character)) = value
        .char_indices()
        .find(|(_, character)| !accepts(*character))
    {
        return Err(NameError::new(
            kind,
            NameErrorReason::InvalidCharacter {
                byte_index,
                character,
            },
        ));
    }
    Ok(())
}

fn validate_logical_segments(kind: NameKind, value: &str) -> Result<(), NameError> {
    if value.starts_with('/') {
        return Err(NameError::new(kind, NameErrorReason::AbsolutePath));
    }
    for (segment_index, segment) in value.split('/').enumerate() {
        let reason = match segment {
            "" => Some(NameErrorReason::EmptySegment { segment_index }),
            "." => Some(NameErrorReason::CurrentDirectorySegment { segment_index }),
            ".." => Some(NameErrorReason::ParentDirectorySegment { segment_index }),
            _ => None,
        };
        if let Some(reason) = reason {
            return Err(NameError::new(kind, reason));
        }
    }
    Ok(())
}

fn split_resource_location(kind: NameKind, value: &str) -> Result<(&str, &str), NameError> {
    value
        .split_once(':')
        .ok_or_else(|| NameError::new(kind, NameErrorReason::MissingNamespaceSeparator))
}

const fn is_namespace_character(character: char) -> bool {
    character.is_ascii_lowercase()
        || character.is_ascii_digit()
        || matches!(character, '_' | '-' | '.')
}

const fn is_resource_path_character(character: char) -> bool {
    is_namespace_character(character) || character == '/'
}

#[cfg(test)]
mod tests {
    use super::{
        DimensionId, FunctionResourceId, FunctionTagResourceId, NameErrorReason, NameKind,
        Namespace, PackNamespace, PackPath, PackResourcePath, ResourcePath, StorageId,
    };
    use crate::target::JavaEditionTarget;

    #[test]
    fn namespace_matches_the_explicit_conservative_target_subset() {
        for accepted in ["minecraft", "a.b-c_0", "a..b", "."] {
            assert_eq!(Namespace::new(accepted).unwrap().as_str(), accepted);
        }

        assert_eq!(
            Namespace::new("").unwrap_err().reason(),
            NameErrorReason::Empty
        );
        assert_eq!(
            Namespace::new("..").unwrap_err().reason(),
            NameErrorReason::ParentDirectorySegment { segment_index: 0 }
        );
        assert_eq!(
            Namespace::new("Upper").unwrap_err().reason(),
            NameErrorReason::InvalidCharacter {
                byte_index: 0,
                character: 'U'
            }
        );
        assert_eq!(
            Namespace::new("a:b").unwrap_err().reason(),
            NameErrorReason::InvalidCharacter {
                byte_index: 1,
                character: ':'
            }
        );
    }

    #[test]
    fn pack_namespace_adds_only_artifact_segment_safety() {
        assert_eq!(PackNamespace::new("a..b").unwrap().as_str(), "a..b");
        assert_eq!(
            PackNamespace::new(".").unwrap_err().reason(),
            NameErrorReason::CurrentDirectorySegment { segment_index: 0 }
        );

        let command_level = Namespace::new("example").unwrap();
        let pack_safe = PackNamespace::try_from(command_level).unwrap();
        let round_trip: Namespace = pack_safe.into();
        assert_eq!(round_trip.as_str(), "example");
    }

    #[test]
    fn resource_path_and_pack_path_have_distinct_invariants() {
        for lexical_only in ["/", "a/", "a//b", ".", "..", "a/../b"] {
            assert!(ResourcePath::new(lexical_only).is_ok(), "{lexical_only}");
            assert!(
                PackResourcePath::new(lexical_only).is_err(),
                "{lexical_only}"
            );
        }

        for accepted in ["foo", "data/foo", "a..b/c.d-e_0", "con"] {
            assert_eq!(PackResourcePath::new(accepted).unwrap().as_str(), accepted);
        }
        assert_eq!(
            PackResourcePath::new("/a").unwrap_err().reason(),
            NameErrorReason::AbsolutePath
        );
        assert_eq!(
            PackResourcePath::new("a//b").unwrap_err().reason(),
            NameErrorReason::EmptySegment { segment_index: 1 }
        );
        assert_eq!(
            PackResourcePath::new("a/./b").unwrap_err().reason(),
            NameErrorReason::CurrentDirectorySegment { segment_index: 1 }
        );
        assert_eq!(
            PackResourcePath::new("a/../b").unwrap_err().reason(),
            NameErrorReason::ParentDirectorySegment { segment_index: 1 }
        );
        assert_eq!(
            ResourcePath::new("café").unwrap_err().reason(),
            NameErrorReason::InvalidCharacter {
                byte_index: 3,
                character: 'é'
            }
        );
    }

    #[test]
    fn logical_pack_paths_reject_every_traversal_shape() {
        let rejected = [
            "",
            "/data/example",
            "data/",
            "data//example",
            "data/./example",
            "data/../example",
            "data\\example",
        ];
        for value in rejected {
            assert!(PackPath::new(value).is_err(), "{value}");
        }

        assert_eq!(PackPath::metadata().as_str(), "pack.mcmeta");
        assert_eq!(
            PackPath::new("data/example/function/con.mcfunction")
                .unwrap()
                .as_str(),
            "data/example/function/con.mcfunction"
        );
    }

    #[test]
    fn resource_kinds_parse_explicit_components_without_aliasing() {
        let function: FunctionResourceId = "example:data/foo".parse().unwrap();
        let tag: FunctionTagResourceId = "example:data/foo".parse().unwrap();
        let storage: StorageId = "example:data/foo".parse().unwrap();
        let dimension: DimensionId = "example:data/foo".parse().unwrap();

        assert_eq!(function.to_string(), "example:data/foo");
        assert_eq!(tag.to_string(), function.to_string());
        assert_eq!(storage.to_string(), function.to_string());
        assert_eq!(dimension.to_string(), function.to_string());
        assert_eq!(function.namespace().as_str(), "example");
        assert_eq!(function.path().as_str(), "data/foo");

        let error = FunctionResourceId::parse("example").unwrap_err();
        assert_eq!(error.kind(), NameKind::FunctionResource);
        assert_eq!(error.reason(), NameErrorReason::MissingNamespaceSeparator);
        assert!(FunctionResourceId::parse(".:foo").is_err());
        assert!(StorageId::parse(".:../foo").is_ok());
    }

    #[test]
    fn function_and_tag_mapping_use_the_target_layout_infallibly() {
        let target = JavaEditionTarget::V26_2;
        let function = FunctionResourceId::parse("example:data/foo").unwrap();
        let tag = FunctionTagResourceId::parse("example:data/foo").unwrap();

        assert_eq!(
            function.pack_path(target).as_str(),
            "data/example/function/data/foo.mcfunction"
        );
        assert_eq!(
            tag.pack_path(target).as_str(),
            "data/example/tags/function/data/foo.json"
        );
    }
}
