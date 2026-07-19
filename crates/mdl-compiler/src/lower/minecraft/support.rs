//! Generated source-method and exact-target support inventory.

use std::fmt::Write as _;

use crate::ir::semantic::{MinecraftSemanticKey, SourceReceiverRule, minecraft_source_methods};
use crate::target::JavaEditionTarget;

use super::MinecraftRecipeId;

/// One supported source spelling, normalized meaning, and exact target recipe.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MinecraftApiSupport {
    source_method: &'static str,
    receiver: SourceReceiverRule,
    semantic_key: MinecraftSemanticKey,
    target: JavaEditionTarget,
    recipe: MinecraftRecipeId,
}

impl MinecraftApiSupport {
    /// Returns the dotted source method spelling.
    #[must_use]
    pub const fn source_method(self) -> &'static str {
        self.source_method
    }

    /// Returns the source receiver rule normalized by the method.
    #[must_use]
    pub const fn receiver(self) -> SourceReceiverRule {
        self.receiver
    }

    /// Returns the normalized target-independent operation identity.
    #[must_use]
    pub const fn semantic_key(self) -> MinecraftSemanticKey {
        self.semantic_key
    }

    /// Returns the exact Minecraft target.
    #[must_use]
    pub const fn target(self) -> JavaEditionTarget {
        self.target
    }

    /// Returns the exact target recipe family.
    #[must_use]
    pub const fn recipe(self) -> MinecraftRecipeId {
        self.recipe
    }
}

/// Generates every supported Minecraft source API row for `target` from the
/// closed method registry and target recipe table.
#[must_use]
pub fn supported_minecraft_api(target: JavaEditionTarget) -> Vec<MinecraftApiSupport> {
    minecraft_source_methods()
        .iter()
        .map(|rule| MinecraftApiSupport {
            source_method: rule.name(),
            receiver: rule.receiver(),
            semantic_key: rule.semantic_key(),
            target,
            recipe: MinecraftRecipeId::for_semantic_key(target, rule.semantic_key()),
        })
        .collect()
}

/// Renders the generated support matrix in stable registry order.
#[must_use]
pub fn dump_supported_minecraft_api(target: JavaEditionTarget) -> String {
    let mut output = String::from("source-method receiver semantic target recipe\n");
    for row in supported_minecraft_api(target) {
        writeln!(
            output,
            "{} {:?} {:?} {} {:?}",
            row.source_method(),
            row.receiver(),
            row.semantic_key(),
            row.target(),
            row.recipe(),
        )
        .expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{dump_supported_minecraft_api, supported_minecraft_api};
    use crate::ir::semantic::{MinecraftSemanticKey, minecraft_source_methods};
    use crate::lower::minecraft::MinecraftRecipeId;
    use crate::target::JavaEditionTarget;

    #[test]
    fn support_rows_are_generated_from_both_closed_registries() {
        let target = JavaEditionTarget::V26_2;
        let rows = supported_minecraft_api(target);
        assert_eq!(rows.len(), minecraft_source_methods().len());
        assert_eq!(rows[0].source_method(), "say");
        assert_eq!(rows[0].semantic_key(), MinecraftSemanticKey::Say);
        assert_eq!(rows[0].target(), target);
        assert_eq!(rows[0].recipe(), MinecraftRecipeId::Java26_2Say);
        assert_eq!(
            dump_supported_minecraft_api(target),
            concat!(
                "source-method receiver semantic target recipe\n",
                "say CurrentExecutor { required_capability: CommandExecutor } Say 26.2 Java26_2Say\n",
                "teleport CurrentExecutor { required_capability: CommandExecutor } TeleportCurrentExecutor 26.2 Java26_2TeleportCurrentExecutor\n",
                "move_by CurrentExecutor { required_capability: CommandExecutor } MoveCurrentExecutorBy 26.2 Java26_2MoveCurrentExecutorBy\n",
                "main_hand_written_book_literal_page_or_empty CurrentExecutor { required_capability: InventoryHolder } ReadMainHandWrittenBookLiteralPage 26.2 Java26_2ReadMainHandWrittenBookLiteralPage\n",
            )
        );
    }
}
