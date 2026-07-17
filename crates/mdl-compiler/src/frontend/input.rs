//! Owned driver-to-frontend package inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

/// One owned in-memory source compilation unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInput {
    name: Box<str>,
    text: Box<str>,
}

impl SourceInput {
    /// Creates one source input with a diagnostic name and UTF-8 text.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, text: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }

    /// Returns the diagnostic source name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the complete source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the input into its diagnostic name and source text.
    #[must_use]
    pub fn into_parts(self) -> (Box<str>, Box<str>) {
        (self.name, self.text)
    }
}

/// Stable driver-owned identity of one module in a compilation graph.
///
/// Keys are opaque to source code. Their text is used only to compare driver input
/// deterministically and to describe malformed graphs.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModuleKey(Box<str>);

impl ModuleKey {
    /// Creates a nonempty opaque module key.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleKeyError::Empty`] for an empty key.
    pub fn new(key: impl Into<Box<str>>) -> Result<Self, ModuleKeyError> {
        let key = key.into();
        if key.is_empty() {
            return Err(ModuleKeyError::Empty);
        }
        Ok(Self(key))
    }

    /// Returns the opaque driver spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn single_source_root() -> Self {
        Self("<single-source-root>".into())
    }
}

impl fmt::Display for ModuleKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Invalid driver-owned module key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleKeyError {
    /// Module identities must not be empty.
    Empty,
}

impl fmt::Display for ModuleKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("module key must not be empty"),
        }
    }
}

impl Error for ModuleKeyError {}

/// Source-visible name through which one module sees a dependency.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ImportName(Box<str>);

impl ImportName {
    /// Creates a nonempty local dependency name.
    ///
    /// The name is matched against a source string literal, so it is not restricted
    /// to identifier syntax.
    ///
    /// # Errors
    ///
    /// Returns [`ImportNameError::Empty`] for an empty name.
    pub fn new(name: impl Into<Box<str>>) -> Result<Self, ImportNameError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ImportNameError::Empty);
        }
        Ok(Self(name))
    }

    /// Returns the source-visible dependency name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ImportName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Invalid source-visible dependency name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportNameError {
    /// Dependency names must not be empty.
    Empty,
}

impl fmt::Display for ImportNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("import name must not be empty"),
        }
    }
}

impl Error for ImportNameError {}

/// One local dependency-name edge in the driver-owned module graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleDependency {
    name: ImportName,
    target: ModuleKey,
}

impl ModuleDependency {
    /// Creates one local dependency edge.
    #[must_use]
    pub const fn new(name: ImportName, target: ModuleKey) -> Self {
        Self { name, target }
    }

    /// Returns the name source code uses with `import`.
    #[must_use]
    pub const fn name(&self) -> &ImportName {
        &self.name
    }

    /// Returns the target module identity.
    #[must_use]
    pub const fn target(&self) -> &ModuleKey {
        &self.target
    }

    /// Consumes the edge into its local name and target.
    #[must_use]
    pub fn into_parts(self) -> (ImportName, ModuleKey) {
        (self.name, self.target)
    }
}

/// One owned module and its complete local dependency map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleInput {
    key: ModuleKey,
    source: SourceInput,
    dependencies: Box<[ModuleDependency]>,
}

impl ModuleInput {
    /// Creates one module input. Graph-wide consistency is checked by package
    /// validation before source ingestion.
    #[must_use]
    pub fn new(
        key: ModuleKey,
        source: SourceInput,
        dependencies: impl Into<Box<[ModuleDependency]>>,
    ) -> Self {
        Self {
            key,
            source,
            dependencies: dependencies.into(),
        }
    }

    /// Returns this module's opaque identity.
    #[must_use]
    pub const fn key(&self) -> &ModuleKey {
        &self.key
    }

    /// Returns its source input.
    #[must_use]
    pub const fn source(&self) -> &SourceInput {
        &self.source
    }

    /// Returns its local dependency edges in their current stored order.
    ///
    /// [`ModuleInput::new`] preserves caller order; [`PackageInput::validate`]
    /// canonicalizes these edges by import name and target.
    #[must_use]
    pub const fn dependencies(&self) -> &[ModuleDependency] {
        &self.dependencies
    }

    /// Consumes this module into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> (ModuleKey, SourceInput, Box<[ModuleDependency]>) {
        (self.key, self.source, self.dependencies)
    }
}

/// Complete owned compilation graph supplied by a driver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInput {
    root: ModuleKey,
    modules: Box<[ModuleInput]>,
}

impl PackageInput {
    /// Creates a package input. Callers may supply modules and dependency edges in
    /// any order; validation canonicalizes them before allocating compiler IDs.
    #[must_use]
    pub fn new(root: ModuleKey, modules: impl Into<Box<[ModuleInput]>>) -> Self {
        Self {
            root,
            modules: modules.into(),
        }
    }

    /// Returns the distinguished root module identity.
    #[must_use]
    pub const fn root(&self) -> &ModuleKey {
        &self.root
    }

    /// Returns modules in their current stored order.
    ///
    /// [`PackageInput::new`] preserves caller order; [`PackageInput::validate`]
    /// canonicalizes modules by [`ModuleKey`].
    #[must_use]
    pub const fn modules(&self) -> &[ModuleInput] {
        &self.modules
    }

    /// Consumes the graph into its root and modules.
    #[must_use]
    pub fn into_parts(self) -> (ModuleKey, Box<[ModuleInput]>) {
        (self.root, self.modules)
    }

    /// Validates and canonicalizes this complete graph.
    ///
    /// Modules are sorted by [`ModuleKey`] and local dependency edges by
    /// [`ImportName`]. Dependency cycles are valid, while missing, duplicate, and
    /// unreachable graph members are rejected.
    ///
    /// # Errors
    ///
    /// Returns the first deterministic [`PackageInputError`] in canonical graph
    /// order.
    pub fn validate(self) -> Result<Self, PackageInputError> {
        let (root, modules) = self.into_parts();
        let mut by_key = BTreeMap::new();
        for mut module in Vec::from(modules) {
            let key = module.key.clone();
            if by_key.contains_key(&key) {
                return Err(PackageInputError::DuplicateModule { key });
            }

            let mut dependencies = Vec::from(module.dependencies);
            dependencies.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.target.cmp(&right.target))
            });
            for duplicate in dependencies.windows(2) {
                if duplicate[0].name == duplicate[1].name {
                    return Err(PackageInputError::DuplicateDependencyName {
                        module: key,
                        name: duplicate[0].name.clone(),
                    });
                }
            }
            module.dependencies = dependencies.into_boxed_slice();
            by_key.insert(key, module);
        }

        if !by_key.contains_key(&root) {
            return Err(PackageInputError::MissingRoot { root });
        }

        for (module_key, module) in &by_key {
            for dependency in &module.dependencies {
                if !by_key.contains_key(&dependency.target) {
                    return Err(PackageInputError::MissingDependencyTarget {
                        module: module_key.clone(),
                        name: dependency.name.clone(),
                        target: dependency.target.clone(),
                    });
                }
            }
        }

        let mut reachable = BTreeSet::new();
        let mut pending = vec![root.clone()];
        while let Some(key) = pending.pop() {
            if !reachable.insert(key.clone()) {
                continue;
            }
            let Some(module) = by_key.get(&key) else {
                // The root and every dependency target were checked above. Keeping
                // traversal total avoids making public validation rely on a panic
                // if this implementation is later rearranged.
                continue;
            };
            pending.extend(
                module
                    .dependencies
                    .iter()
                    .rev()
                    .map(|dependency| dependency.target.clone()),
            );
        }

        if let Some(key) = by_key.keys().find(|key| !reachable.contains(*key)) {
            return Err(PackageInputError::UnreachableModule { key: key.clone() });
        }

        Ok(Self {
            root,
            modules: by_key.into_values().collect::<Vec<_>>().into_boxed_slice(),
        })
    }
}

/// Malformed driver-owned package graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageInputError {
    /// The validated package exceeds the configured module budget.
    ModuleLimitExceeded {
        /// Number of modules supplied by the driver.
        actual: usize,
        /// Maximum module count admitted by the frontend limits.
        limit: usize,
    },
    /// More than one module used the same opaque identity.
    DuplicateModule {
        /// Repeated driver-owned module key.
        key: ModuleKey,
    },
    /// The distinguished root was not supplied.
    MissingRoot {
        /// Requested root key absent from the supplied module graph.
        root: ModuleKey,
    },
    /// Two local dependency edges used the same source-visible name.
    DuplicateDependencyName {
        /// Module that owns both conflicting dependency edges.
        module: ModuleKey,
        /// Repeated source-visible dependency name.
        name: ImportName,
    },
    /// A dependency edge targeted a module absent from the complete graph.
    MissingDependencyTarget {
        /// Module that owns the invalid dependency edge.
        module: ModuleKey,
        /// Source-visible name assigned to the dependency edge.
        name: ImportName,
        /// Missing driver-owned target key.
        target: ModuleKey,
    },
    /// A supplied module was not reachable from the distinguished root.
    UnreachableModule {
        /// Supplied key outside the root's dependency closure.
        key: ModuleKey,
    },
}

impl fmt::Display for PackageInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModuleLimitExceeded { actual, limit } => write!(
                formatter,
                "package contains {actual} modules, exceeding configured limit {limit}"
            ),
            Self::DuplicateModule { key } => {
                write!(formatter, "package contains duplicate module key `{key}`")
            }
            Self::MissingRoot { root } => {
                write!(formatter, "package root `{root}` is not present")
            }
            Self::DuplicateDependencyName { module, name } => write!(
                formatter,
                "module `{module}` contains duplicate dependency name `{name}`"
            ),
            Self::MissingDependencyTarget {
                module,
                name,
                target,
            } => write!(
                formatter,
                "module `{module}` dependency `{name}` targets missing module `{target}`"
            ),
            Self::UnreachableModule { key } => {
                write!(
                    formatter,
                    "module `{key}` is unreachable from the package root"
                )
            }
        }
    }
}

impl Error for PackageInputError {}

#[cfg(test)]
mod tests {
    use super::{
        ImportName, ImportNameError, ModuleDependency, ModuleInput, ModuleKey, ModuleKeyError,
        PackageInput, PackageInputError, SourceInput,
    };

    fn key(value: &str) -> ModuleKey {
        ModuleKey::new(value).unwrap()
    }

    fn name(value: &str) -> ImportName {
        ImportName::new(value).unwrap()
    }

    fn module(key_value: &str, dependencies: Vec<ModuleDependency>) -> ModuleInput {
        ModuleInput::new(
            key(key_value),
            SourceInput::new(format!("{key_value}.mdl"), "fn value() {}"),
            dependencies,
        )
    }

    #[test]
    fn atomic_names_reject_only_the_unusable_empty_identity() {
        assert_eq!(ModuleKey::new(""), Err(ModuleKeyError::Empty));
        assert_eq!(ImportName::new(""), Err(ImportNameError::Empty));
        assert_eq!(key("driver identity").as_str(), "driver identity");
        assert_eq!(name("dependency-name").as_str(), "dependency-name");
    }

    #[test]
    fn validation_canonicalizes_modules_and_local_dependencies() {
        let package = PackageInput::new(
            key("root"),
            vec![
                module("z", vec![]),
                module(
                    "root",
                    vec![
                        ModuleDependency::new(name("z"), key("z")),
                        ModuleDependency::new(name("a"), key("a")),
                    ],
                ),
                module("a", vec![]),
            ],
        );
        let validated = package.validate().unwrap();
        assert_eq!(validated.root().as_str(), "root");
        let (_, modules) = validated.into_parts();
        assert_eq!(
            modules
                .iter()
                .map(|module| module.key().as_str())
                .collect::<Vec<_>>(),
            ["a", "root", "z"]
        );
        assert_eq!(
            modules[1]
                .dependencies()
                .iter()
                .map(|dependency| dependency.name().as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
    }

    #[test]
    fn validation_accepts_dependency_cycles() {
        let package = PackageInput::new(
            key("a"),
            vec![
                module("a", vec![ModuleDependency::new(name("b"), key("b"))]),
                module("b", vec![ModuleDependency::new(name("a"), key("a"))]),
            ],
        );
        assert!(package.validate().is_ok());
    }

    #[test]
    fn validation_rejects_each_graph_ambiguity_deterministically() {
        let duplicate = PackageInput::new(key("a"), vec![module("a", vec![]), module("a", vec![])]);
        assert_eq!(
            duplicate.validate(),
            Err(PackageInputError::DuplicateModule { key: key("a") })
        );

        let missing_root = PackageInput::new(key("root"), vec![module("a", vec![])]);
        assert_eq!(
            missing_root.validate(),
            Err(PackageInputError::MissingRoot { root: key("root") })
        );

        let duplicate_dependency = PackageInput::new(
            key("a"),
            vec![
                module(
                    "a",
                    vec![
                        ModuleDependency::new(name("same"), key("b")),
                        ModuleDependency::new(name("same"), key("c")),
                    ],
                ),
                module("b", vec![]),
                module("c", vec![]),
            ],
        );
        assert_eq!(
            duplicate_dependency.validate(),
            Err(PackageInputError::DuplicateDependencyName {
                module: key("a"),
                name: name("same"),
            })
        );

        let missing_target = PackageInput::new(
            key("a"),
            vec![module(
                "a",
                vec![ModuleDependency::new(name("missing"), key("absent"))],
            )],
        );
        assert_eq!(
            missing_target.validate(),
            Err(PackageInputError::MissingDependencyTarget {
                module: key("a"),
                name: name("missing"),
                target: key("absent"),
            })
        );

        let unreachable = PackageInput::new(
            key("root"),
            vec![module("root", vec![]), module("unused", vec![])],
        );
        assert_eq!(
            unreachable.validate(),
            Err(PackageInputError::UnreachableModule { key: key("unused") })
        );
    }
}
