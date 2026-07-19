use crate::entity::EntityId;
use crate::ir::core::{BlockId, CoreType, FunctionId, InstId, ValueId};
use crate::ir::minecraft::{
    FakeScoreHolder, FunctionResourceId, NbtPath, NbtPathKey, NbtPathSegment, PackResourcePath,
    ResourcePath, StorageId, StoragePath,
};

use super::{LoweringOptions, plan::BranchArm};

/// The sole constructor for compiler-owned Minecraft resources and score homes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GeneratedNames<'a> {
    options: &'a LoweringOptions,
}

impl<'a> GeneratedNames<'a> {
    pub(super) const fn new(options: &'a LoweringOptions) -> Self {
        Self { options }
    }

    pub(crate) fn load_function(self) -> FunctionResourceId {
        self.function_resource("__mdl/load".to_owned())
    }

    pub(crate) fn init_try_create_function(self) -> FunctionResourceId {
        self.function_resource("__mdl/init/try_create".to_owned())
    }

    pub(crate) fn block_function(self, function: FunctionId, block: BlockId) -> FunctionResourceId {
        self.function_resource(format!("__mdl/f{}/b{}", function.index(), block.index()))
    }

    pub(crate) fn branch_helper_function(
        self,
        function: FunctionId,
        source: BlockId,
        arm: BranchArm,
    ) -> FunctionResourceId {
        self.function_resource(format!(
            "__mdl/f{}/e{}_{}",
            function.index(),
            source.index(),
            arm.ordinal()
        ))
    }

    pub(crate) fn external_helper_function(
        self,
        function: FunctionId,
        instruction: InstId,
    ) -> FunctionResourceId {
        self.function_resource(format!(
            "__mdl/f{}/x{}",
            function.index(),
            instruction.index()
        ))
    }

    pub(crate) fn init_sentinel(self) -> StoragePath {
        let objective_hex = encode_hex(self.options.register_objective().as_str().as_bytes());
        let storage = StorageId::new(
            self.options.namespace().as_namespace().clone(),
            ResourcePath::new(&format!("__mdl/init/v0/{objective_hex}"))
                .expect("generated initialization storage path must be valid"),
        );
        let initialized = NbtPathSegment::Key(
            NbtPathKey::new("initialized").expect("generated initialization NBT key must be valid"),
        );
        StoragePath::new(storage, NbtPath::new(initialized, vec![]))
    }

    pub(crate) fn activation_frames_for(
        namespace: &crate::ir::minecraft::PackNamespace,
    ) -> StoragePath {
        let storage = StorageId::new(
            namespace.as_namespace().clone(),
            ResourcePath::new("__mdl/runtime/v0")
                .expect("generated activation storage path must be valid"),
        );
        StoragePath::new(
            storage,
            NbtPath::new(
                NbtPathSegment::Key(
                    NbtPathKey::new("frames").expect("static activation key is valid"),
                ),
                vec![],
            ),
        )
    }

    pub(crate) fn activation_frame_for(base: &StoragePath) -> StoragePath {
        let mut segments = base.path().segments().to_vec();
        segments.push(NbtPathSegment::Index(-1));
        StoragePath::new(
            base.storage().clone(),
            NbtPath::from_segments(segments).expect("activation frame path is nonempty"),
        )
    }

    pub(crate) fn activation_frame_spill_for(
        base: &StoragePath,
        storage_ordinal: u32,
    ) -> StoragePath {
        let mut segments = base.path().segments().to_vec();
        segments.push(NbtPathSegment::Index(-1));
        segments.push(NbtPathSegment::Key(
            NbtPathKey::new(&format!("s{storage_ordinal}"))
                .expect("generated activation spill key must be valid"),
        ));
        StoragePath::new(
            base.storage().clone(),
            NbtPath::from_segments(segments).expect("activation path is nonempty"),
        )
    }

    pub(crate) fn value_holder(function: FunctionId, value: ValueId) -> FakeScoreHolder {
        Self::fake_holder(format!("#f{}v{}", function.index(), value.index()))
    }

    pub(crate) fn result_holder(function: FunctionId, result_index: usize) -> FakeScoreHolder {
        Self::fake_holder(format!("#f{}r{result_index}", function.index()))
    }

    pub(crate) fn edge_temporary_holder(function: FunctionId) -> FakeScoreHolder {
        Self::fake_holder(format!("#f{}t0", function.index()))
    }

    /// Names one Baseline-assigned semantic home by its stable function-local
    /// physical ordinal.
    pub(crate) fn assigned_home_holder(function: FunctionId, ordinal: usize) -> FakeScoreHolder {
        Self::fake_holder(format!("#f{}h{ordinal}", function.index()))
    }

    /// Names fixed-recipe scratch separately from semantic homes and edge scratch.
    pub(crate) fn recipe_temporary_holder(
        function: FunctionId,
        ty: CoreType,
        ordinal: usize,
    ) -> FakeScoreHolder {
        Self::typed_temporary_holder(function, 'q', ty, ordinal)
    }

    /// Names one Baseline edge-cycle scratch home. The type discriminator prevents
    /// the independently allocated Boolean and integer families from colliding.
    pub(crate) fn typed_edge_temporary_holder(
        function: FunctionId,
        ty: CoreType,
        ordinal: usize,
    ) -> FakeScoreHolder {
        Self::typed_temporary_holder(function, 't', ty, ordinal)
    }

    fn typed_temporary_holder(
        function: FunctionId,
        family: char,
        ty: CoreType,
        ordinal: usize,
    ) -> FakeScoreHolder {
        let discriminator = match ty {
            CoreType::Bool => 'b',
            CoreType::I32 => 'i',
            CoreType::ListI32 => 'l',
            CoreType::String => 's',
        };
        Self::fake_holder(format!(
            "#f{}{family}{discriminator}{ordinal}",
            function.index()
        ))
    }

    fn function_resource(self, path: String) -> FunctionResourceId {
        FunctionResourceId::new(
            self.options.namespace().clone(),
            PackResourcePath::try_from(path)
                .expect("generated function resource path must be pack-safe"),
        )
    }

    fn fake_holder(value: String) -> FakeScoreHolder {
        FakeScoreHolder::try_from(value).expect("generated fake score holder must be valid")
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::encode_hex;
    use crate::entity::EntityId;
    use crate::ir::core::{BlockId, CoreProgram, CoreType, FunctionId, ValueId};
    use crate::ir::minecraft::{NbtPathSegment, ObjectiveName, PackNamespace};
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::plan::BranchArm;
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    fn options(objective: &str) -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new(objective).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn resources_and_holders_derive_only_from_typed_dense_ids() {
        let options = options("mdl.reg");
        let names = options.generated_names();
        let function = FunctionId::from_index(2);
        let block = BlockId::from_index(3);
        let value = ValueId::from_index(5);

        assert_eq!(names.load_function().to_string(), "mdl:__mdl/load");
        assert_eq!(
            names.init_try_create_function().to_string(),
            "mdl:__mdl/init/try_create"
        );
        assert_eq!(
            names.block_function(function, block).to_string(),
            "mdl:__mdl/f2/b3"
        );
        assert_eq!(
            super::GeneratedNames::value_holder(function, value).to_string(),
            "#f2v5"
        );
        assert_eq!(
            super::GeneratedNames::result_holder(function, 0).to_string(),
            "#f2r0"
        );
        assert_eq!(
            super::GeneratedNames::edge_temporary_holder(function).to_string(),
            "#f2t0"
        );
        assert_eq!(
            super::GeneratedNames::assigned_home_holder(function, 7).to_string(),
            "#f2h7"
        );
        assert_eq!(
            super::GeneratedNames::recipe_temporary_holder(function, CoreType::Bool, 3).to_string(),
            "#f2qb3"
        );
        assert_eq!(
            super::GeneratedNames::recipe_temporary_holder(function, CoreType::I32, 4).to_string(),
            "#f2qi4"
        );
        assert_eq!(
            super::GeneratedNames::typed_edge_temporary_holder(function, CoreType::Bool, 0)
                .to_string(),
            "#f2tb0"
        );
        assert_eq!(
            super::GeneratedNames::typed_edge_temporary_holder(function, CoreType::I32, 0)
                .to_string(),
            "#f2ti0"
        );
    }

    #[test]
    fn duplicate_diagnostic_name_hints_do_not_affect_resources() {
        let mut program = CoreProgram::new();
        let first = program
            .declare_function(Some("same"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let second = program
            .declare_function(Some("same"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let options = options("mdl.reg");
        let names = options.generated_names();
        let entry = BlockId::from_index(0);

        assert_eq!(program.function(first).unwrap().name_hint(), Some("same"));
        assert_eq!(program.function(second).unwrap().name_hint(), Some("same"));
        assert_eq!(
            names.block_function(first, entry).to_string(),
            "mdl:__mdl/f0/b0"
        );
        assert_eq!(
            names.block_function(second, entry).to_string(),
            "mdl:__mdl/f1/b0"
        );
    }

    #[test]
    fn branch_arms_lower_to_distinct_ordinals_only_at_the_name_boundary() {
        let options = options("mdl.reg");
        let names = options.generated_names();
        let function = FunctionId::from_index(4);
        let source = BlockId::from_index(8);

        assert_eq!(
            names
                .branch_helper_function(function, source, BranchArm::Then)
                .to_string(),
            "mdl:__mdl/f4/e8_0"
        );
        assert_eq!(
            names
                .branch_helper_function(function, source, BranchArm::Else)
                .to_string(),
            "mdl:__mdl/f4/e8_1"
        );
    }

    #[test]
    fn objective_hex_is_reversible_and_storage_is_path_safe() {
        let objective = "Az_-.+09";
        let options = options(objective);
        let sentinel = options.generated_names().init_sentinel();
        let encoded = sentinel
            .storage()
            .path()
            .as_str()
            .rsplit_once('/')
            .unwrap()
            .1;
        let decoded = encoded
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(pair, 16).unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(encoded, encode_hex(objective.as_bytes()));
        assert_eq!(decoded, objective.as_bytes());
        assert_eq!(
            sentinel.storage().to_string(),
            "mdl:__mdl/init/v0/417a5f2d2e2b3039"
        );
        assert!(matches!(
            sentinel.path().segments(),
            [NbtPathSegment::Key(key)] if key.as_str() == "initialized"
        ));
    }

    #[test]
    fn generated_names_are_unique_and_repeatable() {
        let left_options = options("mdl.reg");
        let right_options = left_options.clone();
        let left = left_options.generated_names();
        let right = right_options.generated_names();
        let function = FunctionId::from_index(1);
        let block = BlockId::from_index(2);

        let resources = [
            left.load_function().to_string(),
            left.init_try_create_function().to_string(),
            left.block_function(function, block).to_string(),
            left.branch_helper_function(function, block, BranchArm::Then)
                .to_string(),
            left.branch_helper_function(function, block, BranchArm::Else)
                .to_string(),
        ];
        assert_eq!(
            resources.iter().collect::<HashSet<_>>().len(),
            resources.len()
        );
        assert_eq!(left.load_function(), right.load_function());
        assert_eq!(
            left.block_function(function, block),
            right.block_function(function, block)
        );
        assert_eq!(
            left.init_sentinel().storage(),
            right.init_sentinel().storage()
        );
        assert_eq!(
            left.block_function(function, block)
                .pack_path(left_options.target())
                .as_str(),
            "data/mdl/function/__mdl/f1/b2.mcfunction"
        );
    }
}
