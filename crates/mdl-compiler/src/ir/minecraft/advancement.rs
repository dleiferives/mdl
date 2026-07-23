use super::AdvancementResourceId;

/// One typed `advancement revoke @s only <resource>` command.
///
/// Selector and mode are always `@s`/`only` — the compiler-generated
/// auto-revoke idiom (`notes/compiler/advancement-triggers.md` Part 1.2)
/// never revokes any other target or criterion, so no fields beyond the
/// advancement's own resource are needed.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AdvancementRevokeCommand {
    resource: AdvancementResourceId,
}

impl AdvancementRevokeCommand {
    /// Constructs a self-revoke command for `resource`.
    #[must_use]
    pub const fn new(resource: AdvancementResourceId) -> Self {
        Self { resource }
    }

    /// Returns the advancement this command revokes.
    #[must_use]
    pub const fn resource(&self) -> &AdvancementResourceId {
        &self.resource
    }
}
