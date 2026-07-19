//! Immutable scalar physical-realization and ABI planning.
//!
//! Core values remain semantic SSA identities. This module projects the existing
//! score assignment into separately verified facts, storage declarations,
//! realization occurrences, use requirements, ABI positions, call occurrences, and
//! activation disciplines. Score homes and recursive activation-frame fields are
//! both explicit typed storage declarations rather than hidden emission details.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Write};

use crate::entity::{EntityId, EntityLimitError, EntityVec, entity_id};
use crate::ir::core::{
    BlockId, CoreOp, CoreProgram, CoreType, FunctionId, InstId, TerminatorKind, ValueDef, ValueId,
};
use crate::source::OriginId;

use super::MinecraftOptimizationLevel;
use super::analysis::SemanticInventory;
use super::assignment::{AssignedHomeId, AssignedHomeRole, HomeAssignment};
use super::audit::ActivationOverlapAnalysis;
use super::liveness::LivenessResult;

entity_id!(
    /// Program-wide identity of one logical physical storage declaration.
    pub(crate) struct PhysicalStorageId;
);
entity_id!(
    /// Program-wide identity of one semantic value's availability occurrence.
    pub(crate) struct RealizationId;
);
entity_id!(
    /// Program-wide identity of one exact physical operand occurrence.
    pub(crate) struct UseRequirementId;
);
entity_id!(
    /// Program-wide identity of one selected value materialization.
    pub(crate) struct MaterializationId;
);
entity_id!(
    /// Program-wide identity of one retained Core call occurrence.
    pub(crate) struct CallOccurrenceId;
);

/// Closed scalar storage classes supported by Stage 8.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum PhysicalStorageClass {
    ScoreBool,
    ScoreI32,
    NbtListI32,
    NbtString,
    ActivationNbtBool,
    ActivationNbtI32,
    ActivationNbtListI32,
    ActivationNbtString,
}

impl PhysicalStorageClass {
    pub(crate) const fn primary_for(ty: CoreType) -> Self {
        match ty {
            CoreType::Bool => Self::ScoreBool,
            CoreType::I32 => Self::ScoreI32,
            CoreType::ListI32 => Self::NbtListI32,
            CoreType::String => Self::NbtString,
        }
    }

    pub(crate) const fn core_type(self) -> CoreType {
        match self {
            Self::ScoreBool | Self::ActivationNbtBool => CoreType::Bool,
            Self::ScoreI32 | Self::ActivationNbtI32 => CoreType::I32,
            Self::NbtListI32 | Self::ActivationNbtListI32 => CoreType::ListI32,
            Self::NbtString | Self::ActivationNbtString => CoreType::String,
        }
    }

    pub(crate) const fn activation_frame_for(ty: CoreType) -> Self {
        match ty {
            CoreType::Bool => Self::ActivationNbtBool,
            CoreType::I32 => Self::ActivationNbtI32,
            CoreType::ListI32 => Self::ActivationNbtListI32,
            CoreType::String => Self::ActivationNbtString,
        }
    }
}

/// Compile-time fact about one allocated Core value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValueFact {
    Unknown,
    ConstantBool(bool),
    ConstantI32(i32),
}

/// Logical ownership of one physical storage declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalStorageBinding {
    AssignedScore {
        home: AssignedHomeId,
        role: AssignedHomeRole,
        local_ordinal: usize,
    },
    RecursiveSpill {
        instruction: InstId,
        home: AssignedHomeId,
        spill_index: usize,
    },
}

/// One logical score home or recursive activation-frame field. Concrete target
/// holder/path allocation remains later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalStorageDecl {
    function: FunctionId,
    class: PhysicalStorageClass,
    binding: PhysicalStorageBinding,
}

/// Why one score realization begins to contain its semantic value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RealizationDefinitionSite {
    FunctionParameter {
        parameter_index: usize,
    },
    BlockParameter {
        block: BlockId,
        parameter_index: usize,
    },
    InstructionResult {
        instruction: InstId,
        result_index: usize,
    },
}

/// Sparse-liveness provenance used to validate storage reuse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RealizationLiveRegion {
    /// The value has independently computed sparse liveness segments.
    Sparse {
        segment_start: usize,
        segment_count: usize,
    },
    /// The compatibility policy allocated distinct storage, so conservative whole-
    /// function availability cannot overlap a different realization in that home.
    DistinctStorage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PhysicalLiveSegment {
    block_ordinal: usize,
    from: u64,
    to: u64,
}

/// One semantic value available in one physical storage declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RealizationDecl {
    function: FunctionId,
    value: ValueId,
    storage: PhysicalStorageId,
    ty: CoreType,
    definition: RealizationDefinitionSite,
    live_region: RealizationLiveRegion,
    origin: OriginId,
}

/// Exact point at which a physical recipe reads or defines one semantic value.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum PhysicalUseSite {
    InstructionOperand {
        instruction: InstId,
        operand_index: usize,
    },
    InstructionResult {
        instruction: InstId,
        result_index: usize,
    },
    CallArgument {
        instruction: InstId,
        argument_index: usize,
    },
    CallResult {
        instruction: InstId,
        result_index: usize,
    },
    BranchCondition {
        block: BlockId,
    },
    EdgeArgument {
        block: BlockId,
        successor_index: usize,
        argument_index: usize,
    },
    ReturnValue {
        block: BlockId,
        result_index: usize,
    },
    FunctionParameter {
        parameter_index: usize,
    },
}

/// Recipe timing relative to writes at the same physical occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadTiming {
    BeforeWrites,
    DefinesAtOccurrence,
}

/// One exact semantic use and the realization selected to satisfy it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UseRequirement {
    function: FunctionId,
    value: ValueId,
    site: PhysicalUseSite,
    accepted: Box<[PhysicalStorageClass]>,
    timing: ReadTiming,
    realization: RealizationId,
    destination: Option<PhysicalStorageId>,
    origin: OriginId,
}

/// Selected compatibility materialization recipes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializationRecipe {
    ConstantBoolToScore(bool),
    ConstantI32ToScore(i32),
}

/// A materialization is a physical definition, not a Core instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MaterializationOccurrence {
    function: FunctionId,
    value: ValueId,
    destination: RealizationId,
    site: PhysicalUseSite,
    recipe: MaterializationRecipe,
    origin: OriginId,
}

/// Physical mode of one semantic ABI position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AbiValueMode {
    #[allow(dead_code, reason = "reserved for a later proven ABI elision")]
    ElidedKnown,
    DirectScore {
        storage: PhysicalStorageId,
    },
    DirectNbtValue {
        storage: PhysicalStorageId,
    },
    #[allow(dead_code, reason = "introduced by the recursive-frame tranche")]
    ActivationFrameField {
        storage: PhysicalStorageId,
    },
}

/// One indexed physical ABI position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AbiPosition {
    ty: CoreType,
    mode: AbiValueMode,
}

/// Function ABI is independent of invocation overlap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionAbiPlan {
    parameters: Box<[AbiPosition]>,
    results: Box<[AbiPosition]>,
}

/// Activation lifetime selected independently from ABI value modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivationDiscipline {
    SerialStatic,
    #[allow(dead_code, reason = "introduced by the recursive-frame tranche")]
    RecursiveStack,
}

/// One exact retained call and its indexed semantic/physical transfers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CallOccurrencePlan {
    caller: FunctionId,
    instruction: InstId,
    callee: FunctionId,
    arguments: Box<[(RealizationId, PhysicalStorageId)]>,
    results: Box<[Option<(PhysicalStorageId, RealizationId)>]>,
    activation: ActivationDiscipline,
    /// Caller-owned score homes preserved across this exact recursive edge.
    ///
    /// These remain in assignment identity space until final plan flattening. Keeping
    /// the set on the occurrence makes target construction consume a frozen decision
    /// instead of rediscovering activation storage from the final home inventory.
    spills: Box<[(AssignedHomeId, PhysicalStorageId)]>,
    origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FunctionRealizationPlan {
    facts: Box<[ValueFact]>,
    realizations_by_value: Box<[Box<[RealizationId]>]>,
    assigned_storage_count: usize,
    abi: FunctionAbiPlan,
    activation: ActivationDiscipline,
}

/// Immutable physical-domain plan produced before target resource allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalRealizationPlan {
    level: MinecraftOptimizationLevel,
    storages: EntityVec<PhysicalStorageId, PhysicalStorageDecl>,
    realizations: EntityVec<RealizationId, RealizationDecl>,
    requirements: EntityVec<UseRequirementId, UseRequirement>,
    materializations: EntityVec<MaterializationId, MaterializationOccurrence>,
    calls: EntityVec<CallOccurrenceId, CallOccurrencePlan>,
    functions: EntityVec<FunctionId, FunctionRealizationPlan>,
    live_segments: Box<[PhysicalLiveSegment]>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PhysicalRecipeInventory {
    pub(crate) constant_bool_to_score: usize,
    pub(crate) constant_i32_to_score: usize,
    pub(crate) score_bool_to_frame: usize,
    pub(crate) score_i32_to_frame: usize,
}

/// All-or-nothing admission limits for physical planning tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalPlanningLimits {
    pub(crate) storage_declarations: usize,
    pub(crate) realization_occurrences: usize,
    pub(crate) use_requirements: usize,
    pub(crate) materialization_occurrences: usize,
    pub(crate) call_occurrences: usize,
    pub(crate) live_segments: usize,
    pub(crate) verifier_work: u64,
}

impl PhysicalPlanningLimits {
    pub(crate) const DEFAULT: Self = Self {
        storage_declarations: 1_000_000,
        realization_occurrences: 1_000_000,
        use_requirements: 4_000_000,
        materialization_occurrences: 1_000_000,
        call_occurrences: 1_000_000,
        live_segments: 4_000_000,
        verifier_work: 20_000_000,
    };
}

impl PhysicalRealizationPlan {
    #[allow(
        clippy::too_many_lines,
        reason = "physical planning freezes every aligned scalar table in one all-or-nothing pass"
    )]
    pub(crate) fn for_score_compatibility(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        assignment: &HomeAssignment,
        liveness: Option<&LivenessResult>,
        limits: PhysicalPlanningLimits,
    ) -> Result<Self, PhysicalPlanError> {
        if inventory.len() != core.len() || assignment.len() != core.len() {
            return Err(PhysicalPlanError::PhaseAlignment);
        }
        if !assignment.matches_level(assignment.level())
            || liveness.is_some_and(|facts| !facts.matches_level(assignment.level()))
        {
            return Err(PhysicalPlanError::PolicyMismatch);
        }

        let mut storages = EntityVec::new();
        let mut realizations = EntityVec::new();
        let mut requirements = EntityVec::new();
        let mut materializations = EntityVec::new();
        let mut calls = EntityVec::new();
        let mut functions = EntityVec::new();
        let mut live_segments = Vec::new();
        let activation = ActivationOverlapAnalysis::analyze(core, inventory)
            .map_err(|finding| PhysicalPlanError::ActivationAnalysis(finding.to_string().into()))?;

        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(PhysicalPlanError::MissingDefinition { function })?;
            let assigned = assignment
                .function(function)
                .ok_or(PhysicalPlanError::PhaseAlignment)?;
            let semantic = inventory
                .function(function)
                .ok_or(PhysicalPlanError::PhaseAlignment)?;
            let function_liveness = liveness.and_then(|facts| facts.function(function));

            let mut local_storages = Vec::with_capacity(assigned.homes().len());
            for (local_ordinal, (home, declaration)) in assigned.homes().enumerate() {
                admit(
                    storages.len(),
                    limits.storage_declarations,
                    PhysicalTable::Storages,
                )?;
                let storage = storages
                    .push(PhysicalStorageDecl {
                        function,
                        class: PhysicalStorageClass::primary_for(declaration.ty()),
                        binding: PhysicalStorageBinding::AssignedScore {
                            home,
                            role: declaration.role(),
                            local_ordinal,
                        },
                    })
                    .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
                        table: PhysicalTable::Storages,
                    })?;
                local_storages.push(storage);
            }

            let facts = body
                .values()
                .map(|(value, _)| value_fact(body, value))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let mut realizations_by_value = vec![Vec::new(); body.value_counts().allocated];
            for value in semantic.reachable_values().iter().copied() {
                let Some(value_assignment) = assigned.value_assignment(value) else {
                    continue;
                };
                admit(
                    realizations.len(),
                    limits.realization_occurrences,
                    PhysicalTable::Realizations,
                )?;
                let storage = local_storage(&local_storages, value_assignment.home(), function)?;
                let (definition, origin) = definition_site(body, value, function)?;
                let live_region = retain_live_region(
                    function_liveness.and_then(|facts| facts.completed()),
                    value,
                    &mut live_segments,
                    limits,
                )?;
                let realization = realizations
                    .push(RealizationDecl {
                        function,
                        value,
                        storage,
                        ty: value_assignment.ty(),
                        definition,
                        live_region,
                        origin,
                    })
                    .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
                        table: PhysicalTable::Realizations,
                    })?;
                add_value_realization(&mut realizations_by_value, value, realization, function)?;

                if let Some(recipe) = materialization_recipe(body, value) {
                    admit(
                        materializations.len(),
                        limits.materialization_occurrences,
                        PhysicalTable::Materializations,
                    )?;
                    materializations
                        .push(MaterializationOccurrence {
                            function,
                            value,
                            destination: realization,
                            site: definition_use_site(definition),
                            recipe,
                            origin,
                        })
                        .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
                            table: PhysicalTable::Materializations,
                        })?;
                }
            }

            let abi = build_abi(function, declaration, body, assigned, &local_storages)?;
            build_instruction_requirements(
                function,
                body,
                assigned,
                &local_storages,
                &realizations_by_value,
                &mut requirements,
                &mut calls,
                &mut storages,
                &activation,
                function_liveness.and_then(|facts| facts.completed()),
                limits,
            )?;
            build_terminator_requirements(
                function,
                body,
                assigned,
                &local_storages,
                &realizations_by_value,
                &mut requirements,
                limits,
            )?;
            build_abi_requirements(
                function,
                body,
                assigned,
                &local_storages,
                &realizations_by_value,
                &mut requirements,
                limits,
            )?;

            let inserted = functions
                .push(FunctionRealizationPlan {
                    facts,
                    realizations_by_value: realizations_by_value
                        .into_iter()
                        .map(Vec::into_boxed_slice)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    assigned_storage_count: local_storages.len(),
                    abi,
                    activation: if activation.function_is_recursive(function) {
                        ActivationDiscipline::RecursiveStack
                    } else {
                        ActivationDiscipline::SerialStatic
                    },
                })
                .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
                    table: PhysicalTable::Functions,
                })?;
            if inserted != function {
                return Err(PhysicalPlanError::PhaseAlignment);
            }
        }

        let plan = Self {
            level: assignment.level(),
            storages,
            realizations,
            requirements,
            materializations,
            calls,
            functions,
            live_segments: live_segments.into_boxed_slice(),
        };
        plan.verify(core, inventory, assignment, liveness, limits)?;
        Ok(plan)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the independent verifier audits every physical table relation before publication"
    )]
    pub(crate) fn verify(
        &self,
        core: &CoreProgram,
        inventory: &SemanticInventory,
        assignment: &HomeAssignment,
        liveness: Option<&LivenessResult>,
        limits: PhysicalPlanningLimits,
    ) -> Result<(), PhysicalPlanError> {
        if self.level != assignment.level()
            || self.functions.len() != core.len()
            || inventory.len() != core.len()
        {
            return Err(PhysicalPlanError::PhaseAlignment);
        }
        let activation = ActivationOverlapAnalysis::analyze(core, inventory)
            .map_err(|finding| PhysicalPlanError::ActivationAnalysis(finding.to_string().into()))?;
        let mut work = 0_u64;
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(PhysicalPlanError::MissingDefinition { function })?;
            let assigned = assignment
                .function(function)
                .ok_or(PhysicalPlanError::PhaseAlignment)?;
            let planned = self
                .functions
                .get(function)
                .ok_or(PhysicalPlanError::PhaseAlignment)?;
            charge(&mut work, limits.verifier_work)?;
            if planned.facts.len() != body.value_counts().allocated
                || planned.realizations_by_value.len() != body.value_counts().allocated
                || planned.assigned_storage_count != assigned.homes().len()
                || planned.activation
                    != if activation.function_is_recursive(function) {
                        ActivationDiscipline::RecursiveStack
                    } else {
                        ActivationDiscipline::SerialStatic
                    }
            {
                return Err(PhysicalPlanError::InvalidFunction { function });
            }
            let completed_liveness = liveness
                .and_then(|facts| facts.function(function))
                .and_then(|facts| facts.completed());
            for (value, _) in body.values() {
                charge(&mut work, limits.verifier_work)?;
                if planned.facts[value_index(value)?] != value_fact(body, value) {
                    return Err(PhysicalPlanError::InvalidFact { function, value });
                }
                let expected = assigned.value_assignment(value);
                let actual = &planned.realizations_by_value[value_index(value)?];
                if expected.is_some() == actual.is_empty() {
                    return Err(PhysicalPlanError::InvalidRealization { function, value });
                }
                if let Some(expected) = expected {
                    let (expected_definition, expected_origin) =
                        definition_site(body, value, function)?;
                    for realization in actual.iter().copied() {
                        let actual = self
                            .realizations
                            .get(realization)
                            .ok_or(PhysicalPlanError::InvalidRealization { function, value })?;
                        let storage = self
                            .storages
                            .get(actual.storage)
                            .ok_or(PhysicalPlanError::InvalidRealization { function, value })?;
                        if actual.function != function
                            || actual.value != value
                            || actual.ty != expected.ty()
                            || actual.definition != expected_definition
                            || !live_region_matches(
                                self,
                                actual.live_region,
                                completed_liveness,
                                value,
                            )
                            || actual.origin != expected_origin
                            || storage.function != function
                            || storage.class != PhysicalStorageClass::primary_for(actual.ty)
                            || !matches!(
                                storage.binding,
                                PhysicalStorageBinding::AssignedScore { home, .. }
                                    if home == expected.home()
                            )
                        {
                            return Err(PhysicalPlanError::InvalidRealization { function, value });
                        }
                    }
                }
            }
            verify_storage_nonoverlap(self, function, assigned, liveness, &mut work, limits)?;
            verify_abi(self, function, declaration, assigned)?;
        }
        for (_, requirement) in self.requirements.iter() {
            charge(&mut work, limits.verifier_work)?;
            let realization = self.realizations.get(requirement.realization).ok_or(
                PhysicalPlanError::InvalidRequirement {
                    function: requirement.function,
                    value: requirement.value,
                },
            )?;
            let storage = self.storages.get(realization.storage).ok_or(
                PhysicalPlanError::InvalidRequirement {
                    function: requirement.function,
                    value: requirement.value,
                },
            )?;
            if realization.function != requirement.function
                || realization.value != requirement.value
                || requirement.accepted.as_ref() != [storage.class]
            {
                return Err(PhysicalPlanError::InvalidRequirement {
                    function: requirement.function,
                    value: requirement.value,
                });
            }
            if let Some(destination) = requirement.destination {
                let destination = self.storages.get(destination).ok_or(
                    PhysicalPlanError::InvalidRequirement {
                        function: requirement.function,
                        value: requirement.value,
                    },
                )?;
                if destination.class.core_type() != realization.ty {
                    return Err(PhysicalPlanError::InvalidRequirement {
                        function: requirement.function,
                        value: requirement.value,
                    });
                }
            }
            verify_requirement_details(self, core, assignment, requirement)?;
        }
        verify_complete_requirements(self, core, assignment, &mut work, limits)?;
        for (_, materialization) in self.materializations.iter() {
            charge(&mut work, limits.verifier_work)?;
            let body = core
                .function(materialization.function)
                .and_then(crate::ir::core::Function::body)
                .ok_or(PhysicalPlanError::InvalidMaterialization {
                    function: materialization.function,
                    value: materialization.value,
                })?;
            let realization = self.realizations.get(materialization.destination).ok_or(
                PhysicalPlanError::InvalidMaterialization {
                    function: materialization.function,
                    value: materialization.value,
                },
            )?;
            let (definition, origin) =
                definition_site(body, materialization.value, materialization.function)?;
            let primary = self
                .functions
                .get(materialization.function)
                .and_then(|function| primary_realization(function, materialization.value));
            if realization.function != materialization.function
                || realization.value != materialization.value
                || primary != Some(materialization.destination)
                || materialization_recipe(body, materialization.value)
                    != Some(materialization.recipe)
                || materialization.site != definition_use_site(definition)
                || materialization.origin != origin
            {
                return Err(PhysicalPlanError::InvalidMaterialization {
                    function: materialization.function,
                    value: materialization.value,
                });
            }
        }
        for (_, call) in self.calls.iter() {
            charge(&mut work, limits.verifier_work)?;
            let assigned_caller = assignment
                .function(call.caller)
                .ok_or(PhysicalPlanError::PhaseAlignment)?;
            let caller_body = core
                .function(call.caller)
                .and_then(crate::ir::core::Function::body)
                .ok_or(PhysicalPlanError::InvalidCall {
                    caller: call.caller,
                    instruction: call.instruction,
                })?;
            let instruction_data = caller_body.instruction(call.instruction).ok_or(
                PhysicalPlanError::InvalidCall {
                    caller: call.caller,
                    instruction: call.instruction,
                },
            )?;
            verify_call_transfers(self, call, caller_body, assigned_caller)?;
            verify_recursive_result_separation(self, call, assignment)?;
            let caller_liveness = liveness
                .and_then(|facts| facts.function(call.caller))
                .and_then(|facts| facts.completed());
            let expected_spills = if call.activation == ActivationDiscipline::RecursiveStack {
                exact_recursive_spills(
                    call.caller,
                    caller_body,
                    assigned_caller,
                    caller_liveness,
                    call.instruction,
                )?
            } else {
                Box::new([])
            };
            if call.spills.len() != expected_spills.len() {
                return Err(PhysicalPlanError::InvalidCall {
                    caller: call.caller,
                    instruction: call.instruction,
                });
            }
            for (spill_index, (expected_home, (actual_home, storage))) in expected_spills
                .iter()
                .copied()
                .zip(call.spills.iter().copied())
                .enumerate()
            {
                let expected_ty = assigned_caller
                    .home(expected_home)
                    .ok_or(PhysicalPlanError::InvalidHome {
                        function: call.caller,
                        home: expected_home,
                    })?
                    .ty();
                let declaration =
                    self.storages
                        .get(storage)
                        .ok_or(PhysicalPlanError::InvalidCall {
                            caller: call.caller,
                            instruction: call.instruction,
                        })?;
                if actual_home != expected_home
                    || declaration.function != call.caller
                    || declaration.class != PhysicalStorageClass::activation_frame_for(expected_ty)
                    || declaration.binding
                        != (PhysicalStorageBinding::RecursiveSpill {
                            instruction: call.instruction,
                            home: expected_home,
                            spill_index,
                        })
                {
                    return Err(PhysicalPlanError::InvalidCall {
                        caller: call.caller,
                        instruction: call.instruction,
                    });
                }
            }
            if !matches!(instruction_data.op(), CoreOp::Call(called) if *called == call.callee)
                || call.activation
                    != if activation.call_is_recursive(call.caller, call.instruction) {
                        ActivationDiscipline::RecursiveStack
                    } else {
                        ActivationDiscipline::SerialStatic
                    }
                || call.origin != instruction_data.origin()
            {
                return Err(PhysicalPlanError::InvalidCall {
                    caller: call.caller,
                    instruction: call.instruction,
                });
            }
        }
        verify_storage_declarations(self, assignment, &mut work, limits)?;
        verify_occurrence_completeness(self, core, assignment, &mut work, limits)?;
        verify_live_segment_coverage(self, &mut work, limits)?;
        Ok(())
    }

    pub(crate) fn storage_count(&self) -> usize {
        self.storages.len()
    }

    pub(crate) fn function_assigned_storage_count(&self, function: FunctionId) -> usize {
        self.functions
            .get(function)
            .map_or(0, |function| function.assigned_storage_count)
    }

    pub(crate) fn realization_count(&self) -> usize {
        self.realizations.len()
    }

    pub(crate) fn requirement_count(&self) -> usize {
        self.requirements.len()
    }

    pub(crate) fn materialization_count(&self) -> usize {
        self.materializations.len()
    }

    pub(crate) fn call_count(&self) -> usize {
        self.calls.len()
    }

    pub(crate) fn recursive_call_count(&self) -> usize {
        self.calls
            .iter()
            .filter(|(_, call)| call.activation == ActivationDiscipline::RecursiveStack)
            .count()
    }

    pub(crate) fn recursive_spill_count(&self) -> usize {
        self.calls
            .iter()
            .filter(|(_, call)| call.activation == ActivationDiscipline::RecursiveStack)
            .map(|(_, call)| call.spills.len())
            .sum()
    }

    pub(crate) fn recipe_inventory(&self) -> PhysicalRecipeInventory {
        let mut inventory = PhysicalRecipeInventory::default();
        for (_, materialization) in self.materializations.iter() {
            match materialization.recipe {
                MaterializationRecipe::ConstantBoolToScore(_) => {
                    inventory.constant_bool_to_score += 1;
                }
                MaterializationRecipe::ConstantI32ToScore(_) => {
                    inventory.constant_i32_to_score += 1;
                }
            }
        }
        for (_, call) in self.calls.iter() {
            if call.activation != ActivationDiscipline::RecursiveStack {
                continue;
            }
            for (_, storage) in call.spills.iter().copied() {
                match self
                    .storages
                    .get(storage)
                    .expect("verified spill storage exists")
                    .class
                {
                    PhysicalStorageClass::ActivationNbtBool => {
                        inventory.score_bool_to_frame += 1;
                    }
                    PhysicalStorageClass::ActivationNbtI32
                    | PhysicalStorageClass::ActivationNbtListI32
                    | PhysicalStorageClass::ActivationNbtString => {
                        inventory.score_i32_to_frame += 1;
                    }
                    PhysicalStorageClass::ScoreBool
                    | PhysicalStorageClass::ScoreI32
                    | PhysicalStorageClass::NbtListI32
                    | PhysicalStorageClass::NbtString => {
                        unreachable!("verified recursive spills use activation storage")
                    }
                }
            }
        }
        inventory
    }

    pub(crate) fn call_is_recursive(&self, caller: FunctionId, instruction: InstId) -> bool {
        self.calls.iter().any(|(_, call)| {
            call.caller == caller
                && call.instruction == instruction
                && call.activation == ActivationDiscipline::RecursiveStack
        })
    }

    pub(crate) fn recursive_call_spills(
        &self,
        caller: FunctionId,
        instruction: InstId,
    ) -> Option<impl ExactSizeIterator<Item = (AssignedHomeId, u32, CoreType)> + '_> {
        self.calls.iter().find_map(|(_, call)| {
            (call.caller == caller
                && call.instruction == instruction
                && call.activation == ActivationDiscipline::RecursiveStack)
                .then_some(call.spills.iter().map(|(home, storage)| {
                    let ty = self
                        .storages
                        .get(*storage)
                        .expect("verified recursive spill storage exists")
                        .class
                        .core_type();
                    (*home, storage.index(), ty)
                }))
        })
    }

    pub(crate) fn has_recursive_activation(&self) -> bool {
        self.functions
            .iter()
            .any(|(_, function)| function.activation == ActivationDiscipline::RecursiveStack)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the deterministic audit dump renders each closed physical table in entity order"
    )]
    pub(crate) fn dump(&self) -> String {
        let mut output = String::new();
        writeln!(output, "physical-plan policy={:?}", self.level).unwrap();
        for (storage, declaration) in self.storages.iter() {
            writeln!(
                output,
                "physical-storage {} function={} class={:?} binding={:?}",
                storage.index(),
                declaration.function.index(),
                declaration.class,
                declaration.binding
            )
            .unwrap();
        }
        for (realization, declaration) in self.realizations.iter() {
            writeln!(
                output,
                "realization {} function={} value={} storage={} type={:?} definition={:?} live={:?} origin={:?}",
                realization.index(),
                declaration.function.index(),
                declaration.value.index(),
                declaration.storage.index(),
                declaration.ty,
                declaration.definition,
                declaration.live_region,
                declaration.origin
            )
            .unwrap();
        }
        for (segment, declaration) in self.live_segments.iter().enumerate() {
            writeln!(
                output,
                "physical-live-segment {segment} block={} from={} to={}",
                declaration.block_ordinal, declaration.from, declaration.to
            )
            .unwrap();
        }
        for (requirement, declaration) in self.requirements.iter() {
            writeln!(
                output,
                "physical-use {} function={} value={} site={:?} accepted={:?} timing={:?} realization={} destination={:?} origin={:?}",
                requirement.index(),
                declaration.function.index(),
                declaration.value.index(),
                declaration.site,
                declaration.accepted,
                declaration.timing,
                declaration.realization.index(),
                declaration.destination.map(EntityId::index),
                declaration.origin
            )
            .unwrap();
        }
        for (materialization, declaration) in self.materializations.iter() {
            writeln!(
                output,
                "materialization {} function={} value={} destination={} site={:?} recipe={:?} origin={:?}",
                materialization.index(),
                declaration.function.index(),
                declaration.value.index(),
                declaration.destination.index(),
                declaration.site,
                declaration.recipe,
                declaration.origin
            )
            .unwrap();
        }
        for (function, declaration) in self.functions.iter() {
            let reason = match declaration.activation {
                ActivationDiscipline::SerialStatic => "acyclic-or-synchronously-serial",
                ActivationDiscipline::RecursiveStack => "reachable-recursive-scc",
            };
            writeln!(
                output,
                "physical-function {} activation={:?} reason={reason}",
                function.index(),
                declaration.activation
            )
            .unwrap();
            for (value, (fact, realization)) in declaration
                .facts
                .iter()
                .zip(declaration.realizations_by_value.iter())
                .enumerate()
            {
                writeln!(
                    output,
                    "  fact {value} {:?} realizations={:?} primary={:?}",
                    fact,
                    realization.iter().map(|id| id.index()).collect::<Vec<_>>(),
                    realization.first().map(|id| id.index())
                )
                .unwrap();
            }
            for (index, position) in declaration.abi.parameters.iter().enumerate() {
                writeln!(
                    output,
                    "  abi parameter {index} type={:?} mode={:?}",
                    position.ty, position.mode
                )
                .unwrap();
            }
            for (index, position) in declaration.abi.results.iter().enumerate() {
                writeln!(
                    output,
                    "  abi result {index} type={:?} mode={:?}",
                    position.ty, position.mode
                )
                .unwrap();
            }
        }
        for (call, declaration) in self.calls.iter() {
            let reason = match declaration.activation {
                ActivationDiscipline::SerialStatic => "activations-proven-serial",
                ActivationDiscipline::RecursiveStack => "nested-call-can-overlap-caller",
            };
            writeln!(
                output,
                "physical-call {} caller={} instruction={} callee={} activation={:?} reason={reason} arguments={:?} results={:?} spills={:?} origin={:?}",
                call.index(),
                declaration.caller.index(),
                declaration.instruction.index(),
                declaration.callee.index(),
                declaration.activation,
                declaration.arguments,
                declaration.results,
                declaration.spills,
                declaration.origin
            )
            .unwrap();
        }
        output
    }
}

fn exact_recursive_spills(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    liveness: Option<&super::liveness::CompletedFunctionLiveness>,
    instruction: InstId,
) -> Result<Box<[AssignedHomeId]>, PhysicalPlanError> {
    let liveness = liveness.ok_or(PhysicalPlanError::RecursiveSpillLiveness {
        function,
        instruction,
    })?;
    let mut retained = vec![false; assigned.homes().len()];
    for value_assignment in assigned.value_assignment_slots().iter().flatten() {
        let value = value_assignment.value();
        let defined_by_call = matches!(
            body.value(value).map(crate::ir::core::ValueData::definition),
            Some(ValueDef::InstResult { instruction: defining, .. }) if defining == instruction
        );
        if defined_by_call || !liveness.is_live_after_instruction(body, instruction, value) {
            continue;
        }
        let home = value_assignment.home();
        let index = usize::try_from(home.index())
            .map_err(|_| PhysicalPlanError::InvalidHome { function, home })?;
        *retained
            .get_mut(index)
            .ok_or(PhysicalPlanError::InvalidHome { function, home })? = true;
    }
    Ok(assigned
        .homes()
        .filter_map(|(home, _)| {
            usize::try_from(home.index())
                .ok()
                .and_then(|index| retained.get(index))
                .copied()
                .unwrap_or(false)
                .then_some(home)
        })
        .collect::<Vec<_>>()
        .into_boxed_slice())
}

fn declare_recursive_spill_storages(
    function: FunctionId,
    instruction: InstId,
    assigned: &super::assignment::FunctionHomeAssignment,
    homes: &[AssignedHomeId],
    storages: &mut EntityVec<PhysicalStorageId, PhysicalStorageDecl>,
    limits: PhysicalPlanningLimits,
) -> Result<Box<[(AssignedHomeId, PhysicalStorageId)]>, PhysicalPlanError> {
    homes
        .iter()
        .copied()
        .enumerate()
        .map(|(spill_index, home)| {
            let ty = assigned
                .home(home)
                .ok_or(PhysicalPlanError::InvalidHome { function, home })?
                .ty();
            admit(
                storages.len(),
                limits.storage_declarations,
                PhysicalTable::Storages,
            )?;
            let storage = storages
                .push(PhysicalStorageDecl {
                    function,
                    class: PhysicalStorageClass::activation_frame_for(ty),
                    binding: PhysicalStorageBinding::RecursiveSpill {
                        instruction,
                        home,
                        spill_index,
                    },
                })
                .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
                    table: PhysicalTable::Storages,
                })?;
            Ok((home, storage))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn retain_live_region(
    completed: Option<&super::liveness::CompletedFunctionLiveness>,
    value: ValueId,
    retained: &mut Vec<PhysicalLiveSegment>,
    limits: PhysicalPlanningLimits,
) -> Result<RealizationLiveRegion, PhysicalPlanError> {
    let Some(completed) = completed else {
        return Ok(RealizationLiveRegion::DistinctStorage);
    };
    let segment_start = retained.len();
    for segment in completed.segments(value).iter().copied() {
        admit(
            retained.len(),
            limits.live_segments,
            PhysicalTable::LiveSegments,
        )?;
        let (block_ordinal, from, to) = segment.coordinates();
        retained.push(PhysicalLiveSegment {
            block_ordinal,
            from,
            to,
        });
    }
    Ok(RealizationLiveRegion::Sparse {
        segment_start,
        segment_count: retained.len() - segment_start,
    })
}

fn live_region_matches(
    plan: &PhysicalRealizationPlan,
    actual: RealizationLiveRegion,
    completed: Option<&super::liveness::CompletedFunctionLiveness>,
    value: ValueId,
) -> bool {
    match (actual, completed) {
        (RealizationLiveRegion::DistinctStorage, None) => true,
        (
            RealizationLiveRegion::Sparse {
                segment_start,
                segment_count,
            },
            Some(completed),
        ) => {
            let expected = completed.segments(value);
            segment_count == expected.len()
                && plan
                    .live_segments
                    .get(segment_start..segment_start.saturating_add(segment_count))
                    .is_some_and(|actual| {
                        actual.iter().zip(expected).all(|(actual, expected)| {
                            let (block_ordinal, from, to) = expected.coordinates();
                            *actual
                                == (PhysicalLiveSegment {
                                    block_ordinal,
                                    from,
                                    to,
                                })
                        })
                    })
        }
        (RealizationLiveRegion::DistinctStorage, Some(_))
        | (RealizationLiveRegion::Sparse { .. }, None) => false,
    }
}

fn build_abi(
    function: FunctionId,
    declaration: &crate::ir::core::Function,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    storages: &[PhysicalStorageId],
) -> Result<FunctionAbiPlan, PhysicalPlanError> {
    let parameters = assigned
        .abi()
        .parameters()
        .iter()
        .copied()
        .zip(declaration.parameters().iter().copied())
        .map(|(home, ty)| {
            Ok(AbiPosition {
                ty,
                mode: direct_abi_mode(ty, local_storage(storages, home, function)?),
            })
        })
        .collect::<Result<Vec<_>, PhysicalPlanError>>()?;
    let results = assigned
        .abi()
        .results()
        .iter()
        .copied()
        .zip(declaration.results().iter().copied())
        .map(|(home, ty)| {
            Ok(AbiPosition {
                ty,
                mode: direct_abi_mode(ty, local_storage(storages, home, function)?),
            })
        })
        .collect::<Result<Vec<_>, PhysicalPlanError>>()?;
    if parameters.len() != declaration.parameters().len()
        || results.len() != declaration.results().len()
        || assigned.abi().entry_block() != body.entry()
    {
        return Err(PhysicalPlanError::InvalidAbi { function });
    }
    Ok(FunctionAbiPlan {
        parameters: parameters.into_boxed_slice(),
        results: results.into_boxed_slice(),
    })
}

const fn direct_abi_mode(ty: CoreType, storage: PhysicalStorageId) -> AbiValueMode {
    match ty {
        CoreType::Bool | CoreType::I32 => AbiValueMode::DirectScore { storage },
        CoreType::ListI32 | CoreType::String => AbiValueMode::DirectNbtValue { storage },
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "instruction planning correlates exact operands, results, calls, spills, and use requirements"
)]
fn build_instruction_requirements(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    local_storages: &[PhysicalStorageId],
    realizations: &[Vec<RealizationId>],
    requirements: &mut EntityVec<UseRequirementId, UseRequirement>,
    calls: &mut EntityVec<CallOccurrenceId, CallOccurrencePlan>,
    physical_storages: &mut EntityVec<PhysicalStorageId, PhysicalStorageDecl>,
    activation: &ActivationOverlapAnalysis,
    liveness: Option<&super::liveness::CompletedFunctionLiveness>,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    for (instruction_index, planned) in assigned.instruction_slots().iter().enumerate() {
        let Some(planned) = planned else { continue };
        let instruction = InstId::from_index(
            u32::try_from(instruction_index).map_err(|_| PhysicalPlanError::PhaseAlignment)?,
        );
        let data = body
            .instruction(instruction)
            .ok_or(PhysicalPlanError::InvalidInstruction {
                function,
                instruction,
            })?;
        if let Some(operands) = planned.scalar_operands() {
            if operands.len() != data.operands().len() {
                return Err(PhysicalPlanError::InvalidInstruction {
                    function,
                    instruction,
                });
            }
            for (operand_index, (value, home)) in data
                .operands()
                .iter()
                .copied()
                .zip(operands.iter().copied())
                .enumerate()
            {
                push_requirement(
                    requirements,
                    limits,
                    function,
                    value,
                    PhysicalUseSite::InstructionOperand {
                        instruction,
                        operand_index,
                    },
                    ReadTiming::BeforeWrites,
                    realization_for(realizations, value, function)?,
                    Some(local_storage(local_storages, home, function)?),
                    data.origin(),
                    body,
                )?;
            }
        }
        if let Some(results) = planned.scalar_results() {
            for result in results.iter().copied() {
                let Some(value) = result.semantic_value() else {
                    continue;
                };
                push_requirement(
                    requirements,
                    limits,
                    function,
                    value,
                    PhysicalUseSite::InstructionResult {
                        instruction,
                        result_index: result.result_index(),
                    },
                    ReadTiming::DefinesAtOccurrence,
                    realization_for(realizations, value, function)?,
                    Some(local_storage(local_storages, result.home(), function)?),
                    data.origin(),
                    body,
                )?;
            }
        }
        if let (Some(arguments), Some(result_destinations), CoreOp::Call(callee)) = (
            planned.call_arguments(),
            planned.call_result_destinations(),
            data.op(),
        ) {
            let mut call_arguments = Vec::with_capacity(arguments.len());
            for (argument_index, (value, home)) in data
                .operands()
                .iter()
                .copied()
                .zip(arguments.iter().copied())
                .enumerate()
            {
                let realization = realization_for(realizations, value, function)?;
                let destination = local_storage(local_storages, home, function)?;
                push_requirement(
                    requirements,
                    limits,
                    function,
                    value,
                    PhysicalUseSite::CallArgument {
                        instruction,
                        argument_index,
                    },
                    ReadTiming::BeforeWrites,
                    realization,
                    Some(destination),
                    data.origin(),
                    body,
                )?;
                call_arguments.push((realization, destination));
            }
            let mut call_results = Vec::with_capacity(result_destinations.len());
            for (result_index, destination) in result_destinations.iter().copied().enumerate() {
                let mapped = destination
                    .map(|destination| {
                        let realization =
                            realization_for(realizations, destination.value(), function)?;
                        let storage = local_storage(local_storages, destination.home(), function)?;
                        push_requirement(
                            requirements,
                            limits,
                            function,
                            destination.value(),
                            PhysicalUseSite::CallResult {
                                instruction,
                                result_index,
                            },
                            ReadTiming::DefinesAtOccurrence,
                            realization,
                            Some(storage),
                            data.origin(),
                            body,
                        )?;
                        Ok((storage, realization))
                    })
                    .transpose()?;
                call_results.push(mapped);
            }
            admit(calls.len(), limits.call_occurrences, PhysicalTable::Calls)?;
            let recursive = activation.call_is_recursive(function, instruction);
            let spills = if recursive {
                let homes =
                    exact_recursive_spills(function, body, assigned, liveness, instruction)?;
                declare_recursive_spill_storages(
                    function,
                    instruction,
                    assigned,
                    &homes,
                    physical_storages,
                    limits,
                )?
            } else {
                Box::new([])
            };
            calls
                .push(CallOccurrencePlan {
                    caller: function,
                    instruction,
                    callee: *callee,
                    arguments: call_arguments.into_boxed_slice(),
                    results: call_results.into_boxed_slice(),
                    activation: if recursive {
                        ActivationDiscipline::RecursiveStack
                    } else {
                        ActivationDiscipline::SerialStatic
                    },
                    spills,
                    origin: data.origin(),
                })
                .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
                    table: PhysicalTable::Calls,
                })?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_terminator_requirements(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    storages: &[PhysicalStorageId],
    realizations: &[Vec<RealizationId>],
    requirements: &mut EntityVec<UseRequirementId, UseRequirement>,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    for block in body.block_order().iter().copied() {
        let data = body
            .block(block)
            .ok_or(PhysicalPlanError::InvalidBlock { function, block })?;
        let terminator = data
            .terminator()
            .ok_or(PhysicalPlanError::InvalidBlock { function, block })?;
        if let TerminatorKind::Branch { condition, .. } = terminator.kind() {
            push_requirement(
                requirements,
                limits,
                function,
                *condition,
                PhysicalUseSite::BranchCondition { block },
                ReadTiming::BeforeWrites,
                realization_for(realizations, *condition, function)?,
                None,
                terminator.origin(),
                body,
            )?;
        }
        for (successor_index, target) in terminator.successors().enumerate() {
            let destination =
                body.block(target.block())
                    .ok_or(PhysicalPlanError::InvalidBlock {
                        function,
                        block: target.block(),
                    })?;
            if target.arguments().len() != destination.parameters().len() {
                return Err(PhysicalPlanError::InvalidBlock { function, block });
            }
            for (argument_index, (value, parameter)) in target
                .arguments()
                .iter()
                .copied()
                .zip(destination.parameters())
                .enumerate()
            {
                let Some(source_realization) = optional_realization(realizations, value)? else {
                    continue;
                };
                let Some(destination_assignment) = assigned.value_assignment(parameter.value())
                else {
                    continue;
                };
                let destination_storage = Some(local_storage(
                    storages,
                    destination_assignment.home(),
                    function,
                )?);
                push_requirement(
                    requirements,
                    limits,
                    function,
                    value,
                    PhysicalUseSite::EdgeArgument {
                        block,
                        successor_index,
                        argument_index,
                    },
                    ReadTiming::BeforeWrites,
                    source_realization,
                    destination_storage,
                    terminator.origin(),
                    body,
                )?;
            }
        }
        if let TerminatorKind::Return(values) = terminator.kind() {
            for (result_index, value) in values.iter().copied().enumerate() {
                let destination = assigned
                    .abi()
                    .results()
                    .get(result_index)
                    .copied()
                    .map(|home| local_storage(storages, home, function))
                    .transpose()?;
                push_requirement(
                    requirements,
                    limits,
                    function,
                    value,
                    PhysicalUseSite::ReturnValue {
                        block,
                        result_index,
                    },
                    ReadTiming::BeforeWrites,
                    realization_for(realizations, value, function)?,
                    destination,
                    terminator.origin(),
                    body,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_abi_requirements(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    storages: &[PhysicalStorageId],
    realizations: &[Vec<RealizationId>],
    requirements: &mut EntityVec<UseRequirementId, UseRequirement>,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let entry = body
        .block(body.entry())
        .ok_or(PhysicalPlanError::InvalidBlock {
            function,
            block: body.entry(),
        })?;
    for (parameter_index, (parameter, home)) in entry
        .parameters()
        .iter()
        .zip(assigned.abi().parameters())
        .enumerate()
    {
        push_requirement(
            requirements,
            limits,
            function,
            parameter.value(),
            PhysicalUseSite::FunctionParameter { parameter_index },
            ReadTiming::DefinesAtOccurrence,
            realization_for(realizations, parameter.value(), function)?,
            Some(local_storage(storages, *home, function)?),
            parameter.origin(),
            body,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_requirement(
    requirements: &mut EntityVec<UseRequirementId, UseRequirement>,
    limits: PhysicalPlanningLimits,
    function: FunctionId,
    value: ValueId,
    site: PhysicalUseSite,
    timing: ReadTiming,
    realization: RealizationId,
    destination: Option<PhysicalStorageId>,
    origin: OriginId,
    body: &crate::ir::core::FunctionBody,
) -> Result<(), PhysicalPlanError> {
    let ty = body
        .value(value)
        .ok_or(PhysicalPlanError::InvalidValue { function, value })?
        .ty();
    admit(
        requirements.len(),
        limits.use_requirements,
        PhysicalTable::Requirements,
    )?;
    requirements
        .push(UseRequirement {
            function,
            value,
            site,
            accepted: vec![PhysicalStorageClass::primary_for(ty)].into_boxed_slice(),
            timing,
            realization,
            destination,
            origin,
        })
        .map_err(|EntityLimitError| PhysicalPlanError::EntityLimit {
            table: PhysicalTable::Requirements,
        })?;
    Ok(())
}

fn verify_storage_nonoverlap(
    plan: &PhysicalRealizationPlan,
    function: FunctionId,
    assigned: &super::assignment::FunctionHomeAssignment,
    liveness: Option<&LivenessResult>,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let completed = liveness
        .and_then(|facts| facts.function(function))
        .and_then(|facts| facts.completed());
    let mut values_by_home = vec![Vec::new(); assigned.homes().len()];
    for value in assigned.value_assignment_slots().iter().flatten().copied() {
        charge(work, limits.verifier_work)?;
        let home_index =
            usize::try_from(value.home().index()).map_err(|_| PhysicalPlanError::InvalidHome {
                function,
                home: value.home(),
            })?;
        values_by_home
            .get_mut(home_index)
            .ok_or(PhysicalPlanError::InvalidHome {
                function,
                home: value.home(),
            })?
            .push(value);
    }
    for values in values_by_home {
        if values.len() < 2 {
            continue;
        }
        verify_shared_home_segments(plan, function, &values, completed, work, limits)?;
    }
    Ok(())
}

fn verify_shared_home_segments(
    plan: &PhysicalRealizationPlan,
    function: FunctionId,
    values: &[super::assignment::ValueAssignment],
    completed: Option<&super::liveness::CompletedFunctionLiveness>,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let planned = plan
        .functions
        .get(function)
        .ok_or(PhysicalPlanError::InvalidFunction { function })?;
    let Some(completed) = completed else {
        let expected_ty = values[0].ty();
        for value in values.iter().copied() {
            charge(work, limits.verifier_work)?;
            if value.ty() != expected_ty || primary_realization(planned, value.value()).is_none() {
                return Err(PhysicalPlanError::InvalidRealization {
                    function,
                    value: value.value(),
                });
            }
        }
        // Legacy compatibility constructors do not retain liveness. Production
        // Baseline construction always supplies it and takes the exact sweep below.
        return Ok(());
    };
    let mut segments = Vec::new();
    for value in values.iter().copied() {
        charge(work, limits.verifier_work)?;
        if primary_realization(planned, value.value()).is_none() {
            return Err(PhysicalPlanError::InvalidRealization {
                function,
                value: value.value(),
            });
        }
        for segment in completed.segments(value.value()).iter().copied() {
            charge(work, limits.verifier_work)?;
            let (block, from, to) = segment.coordinates();
            segments.push((block, from, to, value.value()));
        }
    }
    segments.sort_unstable_by_key(|(block, from, to, value)| (*block, *from, *to, value.index()));
    let mut active: Option<(usize, u64, ValueId)> = None;
    for (block, from, to, value) in segments {
        charge(work, limits.verifier_work)?;
        if let Some((active_block, active_to, active_value)) = active {
            if block == active_block && from < active_to && value != active_value {
                return Err(PhysicalPlanError::OverlappingStorage {
                    function,
                    left: active_value,
                    right: value,
                });
            }
            if block != active_block || to > active_to {
                active = Some((block, to, value));
            }
        } else {
            active = Some((block, to, value));
        }
    }
    Ok(())
}

type RequirementKey = (FunctionId, ValueId, PhysicalUseSite);

#[derive(Clone, Copy)]
struct ExpectedRequirementDetails {
    timing: ReadTiming,
    destination: Option<AssignedHomeId>,
    origin: OriginId,
}

fn verify_requirement_details(
    plan: &PhysicalRealizationPlan,
    core: &CoreProgram,
    assignment: &HomeAssignment,
    requirement: &UseRequirement,
) -> Result<(), PhysicalPlanError> {
    let function = requirement.function;
    let value = requirement.value;
    let body = core
        .function(function)
        .and_then(crate::ir::core::Function::body)
        .ok_or(PhysicalPlanError::InvalidRequirement { function, value })?;
    let assigned = assignment
        .function(function)
        .ok_or(PhysicalPlanError::PhaseAlignment)?;
    let planned = plan
        .functions
        .get(function)
        .ok_or(PhysicalPlanError::PhaseAlignment)?;
    if !planned
        .realizations_by_value
        .get(value_index(value)?)
        .is_some_and(|realizations| realizations.contains(&requirement.realization))
    {
        return Err(PhysicalPlanError::InvalidRequirement { function, value });
    }
    let expected = expected_requirement_details(body, assigned, requirement)?;
    let destination_matches = match (expected.destination, requirement.destination) {
        (None, None) => true,
        (Some(expected_home), Some(storage)) => plan.storages.get(storage).is_some_and(|storage| {
            storage.function == function
                && matches!(
                    storage.binding,
                    PhysicalStorageBinding::AssignedScore { home, .. }
                        if home == expected_home
                )
        }),
        (None, Some(_)) | (Some(_), None) => false,
    };
    if requirement.timing != expected.timing
        || requirement.origin != expected.origin
        || !destination_matches
    {
        return Err(PhysicalPlanError::InvalidRequirement { function, value });
    }
    Ok(())
}

fn expected_requirement_details(
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    requirement: &UseRequirement,
) -> Result<ExpectedRequirementDetails, PhysicalPlanError> {
    let details = match requirement.site {
        PhysicalUseSite::InstructionOperand { .. }
        | PhysicalUseSite::InstructionResult { .. }
        | PhysicalUseSite::CallArgument { .. }
        | PhysicalUseSite::CallResult { .. } => {
            expected_instruction_requirement(body, assigned, requirement)?
        }
        PhysicalUseSite::BranchCondition { .. }
        | PhysicalUseSite::EdgeArgument { .. }
        | PhysicalUseSite::ReturnValue { .. }
        | PhysicalUseSite::FunctionParameter { .. } => {
            expected_control_requirement(body, assigned, requirement)?
        }
    };
    if details.destination.is_none()
        && !matches!(requirement.site, PhysicalUseSite::BranchCondition { .. })
    {
        return Err(PhysicalPlanError::InvalidRequirement {
            function: requirement.function,
            value: requirement.value,
        });
    }
    Ok(details)
}

fn expected_instruction_requirement(
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    requirement: &UseRequirement,
) -> Result<ExpectedRequirementDetails, PhysicalPlanError> {
    let invalid = || PhysicalPlanError::InvalidRequirement {
        function: requirement.function,
        value: requirement.value,
    };
    let instruction_plan = |instruction: InstId| {
        assigned
            .instruction_slots()
            .get(usize::try_from(instruction.index()).ok()?)?
            .as_ref()
    };
    let details = match requirement.site {
        PhysicalUseSite::InstructionOperand {
            instruction,
            operand_index,
        } => {
            let data = body.instruction(instruction).ok_or_else(invalid)?;
            if data.operands().get(operand_index) != Some(&requirement.value) {
                return Err(invalid());
            }
            ExpectedRequirementDetails {
                timing: ReadTiming::BeforeWrites,
                destination: instruction_plan(instruction)
                    .and_then(super::assignment::AssignedInstructionPlan::scalar_operands)
                    .and_then(|homes| homes.get(operand_index))
                    .copied(),
                origin: data.origin(),
            }
        }
        PhysicalUseSite::InstructionResult {
            instruction,
            result_index,
        } => {
            let data = body.instruction(instruction).ok_or_else(invalid)?;
            let result = instruction_plan(instruction)
                .and_then(super::assignment::AssignedInstructionPlan::scalar_results)
                .and_then(|results| {
                    results
                        .iter()
                        .copied()
                        .find(|result| result.result_index() == result_index)
                })
                .filter(|result| result.semantic_value() == Some(requirement.value))
                .ok_or_else(invalid)?;
            ExpectedRequirementDetails {
                timing: ReadTiming::DefinesAtOccurrence,
                destination: Some(result.home()),
                origin: data.origin(),
            }
        }
        PhysicalUseSite::CallArgument {
            instruction,
            argument_index,
        } => {
            let data = body.instruction(instruction).ok_or_else(invalid)?;
            if data.operands().get(argument_index) != Some(&requirement.value) {
                return Err(invalid());
            }
            ExpectedRequirementDetails {
                timing: ReadTiming::BeforeWrites,
                destination: instruction_plan(instruction)
                    .and_then(super::assignment::AssignedInstructionPlan::call_arguments)
                    .and_then(|homes| homes.get(argument_index))
                    .copied(),
                origin: data.origin(),
            }
        }
        PhysicalUseSite::CallResult {
            instruction,
            result_index,
        } => {
            let data = body.instruction(instruction).ok_or_else(invalid)?;
            let destination = instruction_plan(instruction)
                .and_then(super::assignment::AssignedInstructionPlan::call_result_destinations)
                .and_then(|destinations| destinations.get(result_index))
                .copied()
                .flatten()
                .filter(|destination| destination.value() == requirement.value)
                .ok_or_else(invalid)?;
            ExpectedRequirementDetails {
                timing: ReadTiming::DefinesAtOccurrence,
                destination: Some(destination.home()),
                origin: data.origin(),
            }
        }
        PhysicalUseSite::BranchCondition { .. }
        | PhysicalUseSite::EdgeArgument { .. }
        | PhysicalUseSite::ReturnValue { .. }
        | PhysicalUseSite::FunctionParameter { .. } => Err(invalid())?,
    };
    Ok(details)
}

fn expected_control_requirement(
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    requirement: &UseRequirement,
) -> Result<ExpectedRequirementDetails, PhysicalPlanError> {
    let invalid = || PhysicalPlanError::InvalidRequirement {
        function: requirement.function,
        value: requirement.value,
    };
    let details = match requirement.site {
        PhysicalUseSite::BranchCondition { block } => {
            let terminator = body
                .block(block)
                .and_then(crate::ir::core::BlockData::terminator)
                .ok_or_else(invalid)?;
            if !matches!(
                terminator.kind(),
                TerminatorKind::Branch { condition, .. } if *condition == requirement.value
            ) {
                return Err(invalid());
            }
            ExpectedRequirementDetails {
                timing: ReadTiming::BeforeWrites,
                destination: None,
                origin: terminator.origin(),
            }
        }
        PhysicalUseSite::EdgeArgument {
            block,
            successor_index,
            argument_index,
        } => {
            let terminator = body
                .block(block)
                .and_then(crate::ir::core::BlockData::terminator)
                .ok_or_else(invalid)?;
            let target = terminator
                .successors()
                .nth(successor_index)
                .ok_or_else(invalid)?;
            if target.arguments().get(argument_index) != Some(&requirement.value) {
                return Err(invalid());
            }
            let parameter = body
                .block(target.block())
                .and_then(|block| block.parameters().get(argument_index))
                .ok_or_else(invalid)?;
            ExpectedRequirementDetails {
                timing: ReadTiming::BeforeWrites,
                destination: assigned
                    .value_assignment(parameter.value())
                    .map(super::assignment::ValueAssignment::home),
                origin: terminator.origin(),
            }
        }
        PhysicalUseSite::ReturnValue {
            block,
            result_index,
        } => {
            let terminator = body
                .block(block)
                .and_then(crate::ir::core::BlockData::terminator)
                .ok_or_else(invalid)?;
            let TerminatorKind::Return(values) = terminator.kind() else {
                return Err(invalid());
            };
            if values.get(result_index) != Some(&requirement.value) {
                return Err(invalid());
            }
            ExpectedRequirementDetails {
                timing: ReadTiming::BeforeWrites,
                destination: assigned.abi().results().get(result_index).copied(),
                origin: terminator.origin(),
            }
        }
        PhysicalUseSite::FunctionParameter { parameter_index } => {
            let parameter = body
                .block(body.entry())
                .and_then(|entry| entry.parameters().get(parameter_index))
                .ok_or_else(invalid)?;
            if parameter.value() != requirement.value {
                return Err(invalid());
            }
            ExpectedRequirementDetails {
                timing: ReadTiming::DefinesAtOccurrence,
                destination: assigned.abi().parameters().get(parameter_index).copied(),
                origin: parameter.origin(),
            }
        }
        PhysicalUseSite::InstructionOperand { .. }
        | PhysicalUseSite::InstructionResult { .. }
        | PhysicalUseSite::CallArgument { .. }
        | PhysicalUseSite::CallResult { .. } => Err(invalid())?,
    };
    Ok(details)
}

fn verify_complete_requirements(
    plan: &PhysicalRealizationPlan,
    core: &CoreProgram,
    assignment: &HomeAssignment,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let mut expected = BTreeSet::new();
    for (function, declaration) in core.functions() {
        let body = declaration
            .body()
            .ok_or(PhysicalPlanError::MissingDefinition { function })?;
        let assigned = assignment
            .function(function)
            .ok_or(PhysicalPlanError::PhaseAlignment)?;
        collect_instruction_requirements(&mut expected, function, body, assigned, work, limits)?;
        collect_control_requirements(&mut expected, function, body, assigned, work, limits)?;
        let entry = body
            .block(body.entry())
            .ok_or(PhysicalPlanError::InvalidBlock {
                function,
                block: body.entry(),
            })?;
        for (parameter_index, parameter) in entry.parameters().iter().enumerate() {
            insert_requirement_key(
                &mut expected,
                (
                    function,
                    parameter.value(),
                    PhysicalUseSite::FunctionParameter { parameter_index },
                ),
            )?;
        }
    }

    let mut actual = BTreeSet::new();
    for (_, requirement) in plan.requirements.iter() {
        charge(work, limits.verifier_work)?;
        insert_requirement_key(
            &mut actual,
            (requirement.function, requirement.value, requirement.site),
        )?;
    }
    if actual != expected {
        return Err(PhysicalPlanError::IncompleteRequirements {
            function: first_requirement_difference(&actual, &expected)
                .map_or(FunctionId::from_index(0), |key| key.0),
        });
    }
    Ok(())
}

fn collect_instruction_requirements(
    expected: &mut BTreeSet<RequirementKey>,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    for (instruction_index, planned) in assigned.instruction_slots().iter().enumerate() {
        charge(work, limits.verifier_work)?;
        let Some(planned) = planned else { continue };
        let instruction = InstId::from_index(
            u32::try_from(instruction_index).map_err(|_| PhysicalPlanError::EntityIndex)?,
        );
        let data = body
            .instruction(instruction)
            .ok_or(PhysicalPlanError::InvalidInstruction {
                function,
                instruction,
            })?;
        if planned.scalar_operands().is_some() {
            for (operand_index, value) in data.operands().iter().copied().enumerate() {
                insert_requirement_key(
                    expected,
                    (
                        function,
                        value,
                        PhysicalUseSite::InstructionOperand {
                            instruction,
                            operand_index,
                        },
                    ),
                )?;
            }
        }
        if let Some(results) = planned.scalar_results() {
            for result in results.iter().copied() {
                let Some(value) = result.semantic_value() else {
                    continue;
                };
                insert_requirement_key(
                    expected,
                    (
                        function,
                        value,
                        PhysicalUseSite::InstructionResult {
                            instruction,
                            result_index: result.result_index(),
                        },
                    ),
                )?;
            }
        }
        if let (Some(arguments), Some(results), CoreOp::Call(_)) = (
            planned.call_arguments(),
            planned.call_result_destinations(),
            data.op(),
        ) {
            if arguments.len() != data.operands().len() {
                return Err(PhysicalPlanError::IncompleteRequirements { function });
            }
            for (argument_index, value) in data.operands().iter().copied().enumerate() {
                insert_requirement_key(
                    expected,
                    (
                        function,
                        value,
                        PhysicalUseSite::CallArgument {
                            instruction,
                            argument_index,
                        },
                    ),
                )?;
            }
            for (result_index, result) in results.iter().copied().enumerate() {
                let Some(result) = result else { continue };
                insert_requirement_key(
                    expected,
                    (
                        function,
                        result.value(),
                        PhysicalUseSite::CallResult {
                            instruction,
                            result_index,
                        },
                    ),
                )?;
            }
        }
    }
    Ok(())
}

fn collect_control_requirements(
    expected: &mut BTreeSet<RequirementKey>,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    for block in body.block_order().iter().copied() {
        charge(work, limits.verifier_work)?;
        let terminator = body
            .block(block)
            .and_then(crate::ir::core::BlockData::terminator)
            .ok_or(PhysicalPlanError::InvalidBlock { function, block })?;
        if let TerminatorKind::Branch { condition, .. } = terminator.kind() {
            insert_requirement_key(
                expected,
                (
                    function,
                    *condition,
                    PhysicalUseSite::BranchCondition { block },
                ),
            )?;
        }
        for (successor_index, target) in terminator.successors().enumerate() {
            let destination =
                body.block(target.block())
                    .ok_or(PhysicalPlanError::InvalidBlock {
                        function,
                        block: target.block(),
                    })?;
            if target.arguments().len() != destination.parameters().len() {
                return Err(PhysicalPlanError::InvalidBlock { function, block });
            }
            for (argument_index, (value, parameter)) in target
                .arguments()
                .iter()
                .copied()
                .zip(destination.parameters())
                .enumerate()
            {
                if assigned.value_assignment(value).is_some()
                    && assigned.value_assignment(parameter.value()).is_some()
                {
                    insert_requirement_key(
                        expected,
                        (
                            function,
                            value,
                            PhysicalUseSite::EdgeArgument {
                                block,
                                successor_index,
                                argument_index,
                            },
                        ),
                    )?;
                }
            }
        }
        if let TerminatorKind::Return(values) = terminator.kind() {
            for (result_index, value) in values.iter().copied().enumerate() {
                insert_requirement_key(
                    expected,
                    (
                        function,
                        value,
                        PhysicalUseSite::ReturnValue {
                            block,
                            result_index,
                        },
                    ),
                )?;
            }
        }
    }
    Ok(())
}

fn insert_requirement_key(
    keys: &mut BTreeSet<RequirementKey>,
    key: RequirementKey,
) -> Result<(), PhysicalPlanError> {
    if keys.insert(key) {
        Ok(())
    } else {
        Err(PhysicalPlanError::IncompleteRequirements { function: key.0 })
    }
}

fn first_requirement_difference<'a>(
    left: &'a BTreeSet<RequirementKey>,
    right: &'a BTreeSet<RequirementKey>,
) -> Option<&'a RequirementKey> {
    left.symmetric_difference(right).next()
}

fn verify_abi(
    plan: &PhysicalRealizationPlan,
    function: FunctionId,
    declaration: &crate::ir::core::Function,
    assigned: &super::assignment::FunctionHomeAssignment,
) -> Result<(), PhysicalPlanError> {
    let abi = &plan
        .functions
        .get(function)
        .ok_or(PhysicalPlanError::InvalidAbi { function })?
        .abi;
    if abi.parameters.len() != declaration.parameters().len()
        || abi.results.len() != declaration.results().len()
    {
        return Err(PhysicalPlanError::InvalidAbi { function });
    }
    for ((position, expected_ty), home) in abi
        .parameters
        .iter()
        .zip(declaration.parameters())
        .zip(assigned.abi().parameters())
        .chain(
            abi.results
                .iter()
                .zip(declaration.results())
                .zip(assigned.abi().results()),
        )
    {
        let storage = match position.mode {
            AbiValueMode::DirectScore { storage }
                if matches!(expected_ty, CoreType::Bool | CoreType::I32) =>
            {
                storage
            }
            AbiValueMode::DirectNbtValue { storage }
                if matches!(expected_ty, CoreType::ListI32 | CoreType::String) =>
            {
                storage
            }
            AbiValueMode::ElidedKnown
            | AbiValueMode::ActivationFrameField { .. }
            | AbiValueMode::DirectScore { .. }
            | AbiValueMode::DirectNbtValue { .. } => {
                return Err(PhysicalPlanError::InvalidAbi { function });
            }
        };
        let storage = plan
            .storages
            .get(storage)
            .ok_or(PhysicalPlanError::InvalidAbi { function })?;
        if position.ty != *expected_ty
            || storage.function != function
            || storage.class != PhysicalStorageClass::primary_for(*expected_ty)
            || !matches!(
                storage.binding,
                PhysicalStorageBinding::AssignedScore {
                    home: actual_home,
                    ..
                } if actual_home == *home
            )
        {
            return Err(PhysicalPlanError::InvalidAbi { function });
        }
    }
    Ok(())
}

fn verify_storage_declarations(
    plan: &PhysicalRealizationPlan,
    assignment: &HomeAssignment,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let mut assigned_seen = Vec::with_capacity(plan.functions.len());
    for (function, _) in plan.functions.iter() {
        charge(work, limits.verifier_work)?;
        let assigned = assignment
            .function(function)
            .ok_or(PhysicalPlanError::InvalidFunction { function })?;
        assigned_seen.push(vec![false; assigned.homes().len()]);
    }
    let mut spill_references = vec![0_usize; plan.storages.len()];
    for (_, call) in plan.calls.iter() {
        charge(work, limits.verifier_work)?;
        for (_, storage) in call.spills.iter().copied() {
            charge(work, limits.verifier_work)?;
            let index =
                usize::try_from(storage.index()).map_err(|_| PhysicalPlanError::InvalidCall {
                    caller: call.caller,
                    instruction: call.instruction,
                })?;
            let references =
                spill_references
                    .get_mut(index)
                    .ok_or(PhysicalPlanError::InvalidCall {
                        caller: call.caller,
                        instruction: call.instruction,
                    })?;
            *references = references
                .checked_add(1)
                .ok_or(PhysicalPlanError::InvalidCall {
                    caller: call.caller,
                    instruction: call.instruction,
                })?;
        }
    }
    for (storage_id, storage) in plan.storages.iter() {
        charge(work, limits.verifier_work)?;
        let function_index = usize::try_from(storage.function.index()).map_err(|_| {
            PhysicalPlanError::InvalidStorage {
                storage: storage_id,
            }
        })?;
        let assigned =
            assignment
                .function(storage.function)
                .ok_or(PhysicalPlanError::InvalidStorage {
                    storage: storage_id,
                })?;
        match storage.binding {
            PhysicalStorageBinding::AssignedScore {
                home,
                role,
                local_ordinal,
            } => {
                let Some((expected_home, expected)) = assigned.homes().nth(local_ordinal) else {
                    return Err(PhysicalPlanError::InvalidStorage {
                        storage: storage_id,
                    });
                };
                let seen = assigned_seen
                    .get_mut(function_index)
                    .and_then(|seen| seen.get_mut(local_ordinal))
                    .ok_or(PhysicalPlanError::InvalidStorage {
                        storage: storage_id,
                    })?;
                if expected_home != home
                    || expected.role() != role
                    || storage.class != PhysicalStorageClass::primary_for(expected.ty())
                    || *seen
                {
                    return Err(PhysicalPlanError::InvalidStorage {
                        storage: storage_id,
                    });
                }
                *seen = true;
            }
            PhysicalStorageBinding::RecursiveSpill { home, .. } => {
                let expected = assigned
                    .home(home)
                    .ok_or(PhysicalPlanError::InvalidStorage {
                        storage: storage_id,
                    })?;
                let storage_index = usize::try_from(storage_id.index()).map_err(|_| {
                    PhysicalPlanError::InvalidStorage {
                        storage: storage_id,
                    }
                })?;
                if storage.class != PhysicalStorageClass::activation_frame_for(expected.ty())
                    || spill_references.get(storage_index).copied() != Some(1)
                {
                    return Err(PhysicalPlanError::InvalidStorage {
                        storage: storage_id,
                    });
                }
            }
        }
    }
    if assigned_seen.iter().flatten().any(|seen| !seen) {
        return Err(PhysicalPlanError::PhaseAlignment);
    }
    Ok(())
}

fn verify_occurrence_completeness(
    plan: &PhysicalRealizationPlan,
    core: &CoreProgram,
    assignment: &HomeAssignment,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    verify_realization_and_materialization_completeness(plan, work, limits)?;
    verify_call_completeness(plan, core, assignment, work, limits)
}

fn verify_realization_and_materialization_completeness(
    plan: &PhysicalRealizationPlan,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let mut realization_owners = vec![0_u8; plan.realizations.len()];
    let mut expected_materializations = BTreeSet::new();
    for (function, planned) in plan.functions.iter() {
        for (value_index, (fact, realizations)) in planned
            .facts
            .iter()
            .zip(planned.realizations_by_value.iter())
            .enumerate()
        {
            charge(work, limits.verifier_work)?;
            let value = ValueId::from_index(
                u32::try_from(value_index).map_err(|_| PhysicalPlanError::EntityIndex)?,
            );
            for realization in realizations.iter().copied() {
                let index = usize::try_from(realization.index())
                    .map_err(|_| PhysicalPlanError::DetachedRealization { realization })?;
                let owners = realization_owners
                    .get_mut(index)
                    .ok_or(PhysicalPlanError::DetachedRealization { realization })?;
                *owners = owners.saturating_add(1);
            }
            if !realizations.is_empty() && !matches!(fact, ValueFact::Unknown) {
                expected_materializations.insert((function, value));
            }
        }
    }
    if let Some((index, _)) = realization_owners
        .iter()
        .enumerate()
        .find(|(_, owners)| **owners != 1)
    {
        let realization = RealizationId::from_index(
            u32::try_from(index).map_err(|_| PhysicalPlanError::EntityIndex)?,
        );
        return Err(PhysicalPlanError::DetachedRealization { realization });
    }

    let mut actual_materializations = BTreeSet::new();
    for (_, occurrence) in plan.materializations.iter() {
        charge(work, limits.verifier_work)?;
        if !actual_materializations.insert((occurrence.function, occurrence.value)) {
            return Err(PhysicalPlanError::IncompleteMaterializations {
                function: occurrence.function,
            });
        }
    }
    if actual_materializations != expected_materializations {
        let function = actual_materializations
            .symmetric_difference(&expected_materializations)
            .next()
            .map_or(FunctionId::from_index(0), |(function, _)| *function);
        return Err(PhysicalPlanError::IncompleteMaterializations { function });
    }
    Ok(())
}

fn verify_call_completeness(
    plan: &PhysicalRealizationPlan,
    core: &CoreProgram,
    assignment: &HomeAssignment,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let mut expected_calls = BTreeSet::new();
    for (function, declaration) in core.functions() {
        let body = declaration
            .body()
            .ok_or(PhysicalPlanError::MissingDefinition { function })?;
        let assigned = assignment
            .function(function)
            .ok_or(PhysicalPlanError::PhaseAlignment)?;
        for (instruction_index, occurrence) in assigned.instruction_slots().iter().enumerate() {
            charge(work, limits.verifier_work)?;
            let Some(occurrence) = occurrence else {
                continue;
            };
            if occurrence.call_arguments().is_none() {
                continue;
            }
            let instruction = InstId::from_index(
                u32::try_from(instruction_index).map_err(|_| PhysicalPlanError::EntityIndex)?,
            );
            if !matches!(
                body.instruction(instruction)
                    .map(crate::ir::core::InstData::op),
                Some(CoreOp::Call(_))
            ) || !expected_calls.insert((function, instruction))
            {
                return Err(PhysicalPlanError::IncompleteCalls { function });
            }
        }
    }
    let mut actual_calls = BTreeSet::new();
    for (_, call) in plan.calls.iter() {
        charge(work, limits.verifier_work)?;
        if !actual_calls.insert((call.caller, call.instruction)) {
            return Err(PhysicalPlanError::IncompleteCalls {
                function: call.caller,
            });
        }
    }
    if actual_calls != expected_calls {
        let function = actual_calls
            .symmetric_difference(&expected_calls)
            .next()
            .map_or(FunctionId::from_index(0), |(function, _)| *function);
        return Err(PhysicalPlanError::IncompleteCalls { function });
    }
    Ok(())
}

fn verify_live_segment_coverage(
    plan: &PhysicalRealizationPlan,
    work: &mut u64,
    limits: PhysicalPlanningLimits,
) -> Result<(), PhysicalPlanError> {
    let mut covered = vec![false; plan.live_segments.len()];
    for (_, realization) in plan.realizations.iter() {
        let RealizationLiveRegion::Sparse {
            segment_start,
            segment_count,
        } = realization.live_region
        else {
            continue;
        };
        let end = segment_start.checked_add(segment_count).ok_or(
            PhysicalPlanError::InvalidLiveSegment {
                segment: segment_start,
            },
        )?;
        let region =
            covered
                .get_mut(segment_start..end)
                .ok_or(PhysicalPlanError::InvalidLiveSegment {
                    segment: segment_start,
                })?;
        for referenced in region {
            charge(work, limits.verifier_work)?;
            *referenced = true;
        }
    }
    if let Some(segment) = covered.iter().position(|covered| !covered) {
        return Err(PhysicalPlanError::InvalidLiveSegment { segment });
    }
    Ok(())
}

fn verify_recursive_result_separation(
    plan: &PhysicalRealizationPlan,
    call: &CallOccurrencePlan,
    assignment: &HomeAssignment,
) -> Result<(), PhysicalPlanError> {
    if call.activation != ActivationDiscipline::RecursiveStack {
        return Ok(());
    }
    let invalid = || PhysicalPlanError::InvalidCall {
        caller: call.caller,
        instruction: call.instruction,
    };
    let spill_homes = call
        .spills
        .iter()
        .map(|(home, _)| *home)
        .collect::<BTreeSet<_>>();
    for result in call.results.iter().flatten() {
        let storage = plan.storages.get(result.0).ok_or_else(invalid)?;
        let PhysicalStorageBinding::AssignedScore { home, .. } = storage.binding else {
            return Err(invalid());
        };
        if spill_homes.contains(&home) {
            return Err(invalid());
        }
    }
    if call.caller == call.callee {
        let callee = assignment.function(call.callee).ok_or_else(invalid)?;
        if callee
            .abi()
            .results()
            .iter()
            .any(|home| spill_homes.contains(home))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn verify_call_transfers(
    plan: &PhysicalRealizationPlan,
    call: &CallOccurrencePlan,
    body: &crate::ir::core::FunctionBody,
    assigned: &super::assignment::FunctionHomeAssignment,
) -> Result<(), PhysicalPlanError> {
    let invalid = || PhysicalPlanError::InvalidCall {
        caller: call.caller,
        instruction: call.instruction,
    };
    let data = body.instruction(call.instruction).ok_or_else(invalid)?;
    if !matches!(data.op(), CoreOp::Call(callee) if *callee == call.callee) {
        return Err(invalid());
    }
    let instruction_index = usize::try_from(call.instruction.index()).map_err(|_| invalid())?;
    let occurrence = assigned
        .instruction_slots()
        .get(instruction_index)
        .and_then(Option::as_ref)
        .ok_or_else(invalid)?;
    let arguments = occurrence.call_arguments().ok_or_else(invalid)?;
    let results = occurrence.call_result_destinations().ok_or_else(invalid)?;
    if data.operands().len() != arguments.len()
        || call.arguments.len() != arguments.len()
        || call.results.len() != results.len()
    {
        return Err(invalid());
    }
    let planned = plan.functions.get(call.caller).ok_or_else(invalid)?;
    for ((value, home), (realization, storage)) in data
        .operands()
        .iter()
        .copied()
        .zip(arguments.iter().copied())
        .zip(call.arguments.iter().copied())
    {
        if primary_realization(planned, value) != Some(realization)
            || !assigned_storage_matches(plan, storage, call.caller, home)
        {
            return Err(invalid());
        }
    }
    for (expected, actual) in results.iter().copied().zip(call.results.iter().copied()) {
        match (expected, actual) {
            (None, None) => {}
            (Some(expected), Some((storage, realization)))
                if primary_realization(planned, expected.value()) == Some(realization)
                    && assigned_storage_matches(plan, storage, call.caller, expected.home()) => {}
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

fn primary_realization(planned: &FunctionRealizationPlan, value: ValueId) -> Option<RealizationId> {
    planned
        .realizations_by_value
        .get(value_index(value).ok()?)
        .and_then(|realizations| realizations.first())
        .copied()
}

fn assigned_storage_matches(
    plan: &PhysicalRealizationPlan,
    storage: PhysicalStorageId,
    function: FunctionId,
    expected_home: AssignedHomeId,
) -> bool {
    plan.storages.get(storage).is_some_and(|storage| {
        storage.function == function
            && matches!(
                storage.binding,
                PhysicalStorageBinding::AssignedScore { home, .. } if home == expected_home
            )
    })
}

fn value_fact(body: &crate::ir::core::FunctionBody, value: ValueId) -> ValueFact {
    let Some(data) = body.value(value) else {
        return ValueFact::Unknown;
    };
    let ValueDef::InstResult { instruction, .. } = data.definition() else {
        return ValueFact::Unknown;
    };
    match body
        .instruction(instruction)
        .map(crate::ir::core::InstData::op)
    {
        Some(CoreOp::BoolConstant(value)) => ValueFact::ConstantBool(*value),
        Some(CoreOp::I32Constant(value)) => ValueFact::ConstantI32(*value),
        _ => ValueFact::Unknown,
    }
}

fn materialization_recipe(
    body: &crate::ir::core::FunctionBody,
    value: ValueId,
) -> Option<MaterializationRecipe> {
    match value_fact(body, value) {
        ValueFact::Unknown => None,
        ValueFact::ConstantBool(value) => Some(MaterializationRecipe::ConstantBoolToScore(value)),
        ValueFact::ConstantI32(value) => Some(MaterializationRecipe::ConstantI32ToScore(value)),
    }
}

fn definition_site(
    body: &crate::ir::core::FunctionBody,
    value: ValueId,
    function: FunctionId,
) -> Result<(RealizationDefinitionSite, OriginId), PhysicalPlanError> {
    let value_data = body
        .value(value)
        .ok_or(PhysicalPlanError::InvalidValue { function, value })?;
    match value_data.definition() {
        ValueDef::BlockParam {
            block,
            parameter_index,
        } => {
            let index = usize::try_from(parameter_index)
                .map_err(|_| PhysicalPlanError::InvalidValue { function, value })?;
            let parameter = body
                .block(block)
                .and_then(|data| data.parameters().get(index))
                .ok_or(PhysicalPlanError::InvalidValue { function, value })?;
            let site = if block == body.entry() {
                RealizationDefinitionSite::FunctionParameter {
                    parameter_index: index,
                }
            } else {
                RealizationDefinitionSite::BlockParameter {
                    block,
                    parameter_index: index,
                }
            };
            Ok((site, parameter.origin()))
        }
        ValueDef::InstResult {
            instruction,
            result_index,
        } => {
            let data = body
                .instruction(instruction)
                .ok_or(PhysicalPlanError::InvalidValue { function, value })?;
            Ok((
                RealizationDefinitionSite::InstructionResult {
                    instruction,
                    result_index: usize::try_from(result_index)
                        .map_err(|_| PhysicalPlanError::InvalidValue { function, value })?,
                },
                data.origin(),
            ))
        }
    }
}

const fn definition_use_site(definition: RealizationDefinitionSite) -> PhysicalUseSite {
    match definition {
        RealizationDefinitionSite::FunctionParameter { parameter_index } => {
            PhysicalUseSite::FunctionParameter { parameter_index }
        }
        RealizationDefinitionSite::BlockParameter {
            block,
            parameter_index,
        } => PhysicalUseSite::EdgeArgument {
            block,
            successor_index: 0,
            argument_index: parameter_index,
        },
        RealizationDefinitionSite::InstructionResult {
            instruction,
            result_index,
        } => PhysicalUseSite::InstructionResult {
            instruction,
            result_index,
        },
    }
}

fn local_storage(
    storages: &[PhysicalStorageId],
    home: AssignedHomeId,
    function: FunctionId,
) -> Result<PhysicalStorageId, PhysicalPlanError> {
    storages
        .get(
            usize::try_from(home.index())
                .map_err(|_| PhysicalPlanError::InvalidHome { function, home })?,
        )
        .copied()
        .ok_or(PhysicalPlanError::InvalidHome { function, home })
}

fn realization_for(
    realizations: &[Vec<RealizationId>],
    value: ValueId,
    function: FunctionId,
) -> Result<RealizationId, PhysicalPlanError> {
    realizations
        .get(value_index(value)?)
        .and_then(|realizations| realizations.first())
        .copied()
        .ok_or(PhysicalPlanError::InvalidRealization { function, value })
}

fn optional_realization(
    realizations: &[Vec<RealizationId>],
    value: ValueId,
) -> Result<Option<RealizationId>, PhysicalPlanError> {
    realizations
        .get(value_index(value)?)
        .map(|realizations| realizations.first().copied())
        .ok_or(PhysicalPlanError::EntityIndex)
}

fn add_value_realization(
    slots: &mut [Vec<RealizationId>],
    value: ValueId,
    realization: RealizationId,
    function: FunctionId,
) -> Result<(), PhysicalPlanError> {
    let slot = slots
        .get_mut(value_index(value)?)
        .ok_or(PhysicalPlanError::InvalidValue { function, value })?;
    if slot.contains(&realization) {
        return Err(PhysicalPlanError::InvalidRealization { function, value });
    }
    slot.push(realization);
    Ok(())
}

fn value_index(value: ValueId) -> Result<usize, PhysicalPlanError> {
    usize::try_from(value.index()).map_err(|_| PhysicalPlanError::EntityIndex)
}

fn admit(current: usize, limit: usize, table: PhysicalTable) -> Result<(), PhysicalPlanError> {
    if current >= limit {
        Err(PhysicalPlanError::Limit { table, limit })
    } else {
        Ok(())
    }
}

fn charge(work: &mut u64, limit: u64) -> Result<(), PhysicalPlanError> {
    *work = work
        .checked_add(1)
        .ok_or(PhysicalPlanError::VerifierLimit { limit })?;
    if *work > limit {
        Err(PhysicalPlanError::VerifierLimit { limit })
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalTable {
    Functions,
    Storages,
    Realizations,
    LiveSegments,
    Requirements,
    Materializations,
    Calls,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalPlanError {
    PhaseAlignment,
    PolicyMismatch,
    MissingDefinition {
        function: FunctionId,
    },
    InvalidFunction {
        function: FunctionId,
    },
    InvalidBlock {
        function: FunctionId,
        block: BlockId,
    },
    InvalidInstruction {
        function: FunctionId,
        instruction: InstId,
    },
    InvalidValue {
        function: FunctionId,
        value: ValueId,
    },
    InvalidHome {
        function: FunctionId,
        home: AssignedHomeId,
    },
    InvalidStorage {
        storage: PhysicalStorageId,
    },
    InvalidFact {
        function: FunctionId,
        value: ValueId,
    },
    InvalidRealization {
        function: FunctionId,
        value: ValueId,
    },
    DetachedRealization {
        realization: RealizationId,
    },
    InvalidLiveSegment {
        segment: usize,
    },
    InvalidRequirement {
        function: FunctionId,
        value: ValueId,
    },
    IncompleteRequirements {
        function: FunctionId,
    },
    IncompleteMaterializations {
        function: FunctionId,
    },
    IncompleteCalls {
        function: FunctionId,
    },
    InvalidMaterialization {
        function: FunctionId,
        value: ValueId,
    },
    InvalidAbi {
        function: FunctionId,
    },
    InvalidCall {
        caller: FunctionId,
        instruction: InstId,
    },
    RecursiveSpillLiveness {
        function: FunctionId,
        instruction: InstId,
    },
    OverlappingStorage {
        function: FunctionId,
        left: ValueId,
        right: ValueId,
    },
    Limit {
        table: PhysicalTable,
        limit: usize,
    },
    VerifierLimit {
        limit: u64,
    },
    EntityLimit {
        table: PhysicalTable,
    },
    EntityIndex,
    ActivationAnalysis(Box<str>),
}

impl fmt::Display for PhysicalPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PhaseAlignment => {
                formatter.write_str("physical-plan phase inputs are misaligned")
            }
            Self::PolicyMismatch => formatter.write_str("physical-plan policies disagree"),
            Self::MissingDefinition { function } => write!(formatter, "{function:?} has no body"),
            Self::InvalidFunction { function } => {
                write!(formatter, "invalid physical plan for {function:?}")
            }
            Self::InvalidBlock { function, block } => {
                write!(formatter, "invalid physical block {function:?} {block:?}")
            }
            Self::InvalidInstruction {
                function,
                instruction,
            } => write!(
                formatter,
                "invalid physical instruction {function:?} {instruction:?}"
            ),
            Self::InvalidValue { function, value } => {
                write!(formatter, "invalid physical value {function:?} {value:?}")
            }
            Self::InvalidHome { function, home } => {
                write!(formatter, "invalid physical home {function:?} {home:?}")
            }
            Self::InvalidStorage { storage } => {
                write!(formatter, "invalid physical storage {storage:?}")
            }
            Self::InvalidFact { function, value } => {
                write!(formatter, "invalid value fact {function:?} {value:?}")
            }
            Self::InvalidRealization { function, value } => {
                write!(formatter, "invalid realization {function:?} {value:?}")
            }
            Self::DetachedRealization { realization } => {
                write!(formatter, "detached physical realization {realization:?}")
            }
            Self::InvalidLiveSegment { segment } => {
                write!(formatter, "invalid physical live segment {segment}")
            }
            Self::InvalidRequirement { function, value } => {
                write!(formatter, "invalid use requirement {function:?} {value:?}")
            }
            Self::IncompleteRequirements { function } => {
                write!(
                    formatter,
                    "incomplete physical use requirements for {function:?}"
                )
            }
            Self::IncompleteMaterializations { function } => {
                write!(
                    formatter,
                    "incomplete physical materializations for {function:?}"
                )
            }
            Self::IncompleteCalls { function } => {
                write!(formatter, "incomplete physical calls for {function:?}")
            }
            Self::InvalidMaterialization { function, value } => {
                write!(formatter, "invalid materialization {function:?} {value:?}")
            }
            Self::InvalidAbi { function } => {
                write!(formatter, "invalid physical ABI for {function:?}")
            }
            Self::InvalidCall {
                caller,
                instruction,
            } => write!(
                formatter,
                "invalid call occurrence {caller:?} {instruction:?}"
            ),
            Self::RecursiveSpillLiveness {
                function,
                instruction,
            } => write!(
                formatter,
                "recursive spill planning requires complete liveness for {function:?} {instruction:?}"
            ),
            Self::OverlappingStorage {
                function,
                left,
                right,
            } => write!(
                formatter,
                "overlapping realizations in {function:?}: {left:?} and {right:?}"
            ),
            Self::Limit { table, limit } => {
                write!(formatter, "physical {table:?} limit {limit} exhausted")
            }
            Self::VerifierLimit { limit } => {
                write!(formatter, "physical verifier work limit {limit} exhausted")
            }
            Self::EntityLimit { table } => {
                write!(formatter, "physical {table:?} identity space exhausted")
            }
            Self::EntityIndex => formatter.write_str("physical entity index does not fit the host"),
            Self::ActivationAnalysis(message) => {
                write!(formatter, "activation analysis failed: {message}")
            }
        }
    }
}

impl Error for PhysicalPlanError {}

#[cfg(test)]
mod tests {
    use crate::ir::core::{CoreProgram, FunctionBuilder, Terminator, TerminatorKind};
    use crate::source::{OriginId, SourceContext};

    use super::*;

    fn score_plan() -> (
        CoreProgram,
        SemanticInventory,
        HomeAssignment,
        PhysicalRealizationPlan,
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("main"), vec![], vec![CoreType::I32], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(20, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(22, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let plan = PhysicalRealizationPlan::for_score_compatibility(
            &core,
            &inventory,
            &assignment,
            None,
            PhysicalPlanningLimits::DEFAULT,
        )
        .unwrap();
        (core, inventory, assignment, plan)
    }

    fn sparse_score_plan() -> (
        CoreProgram,
        SemanticInventory,
        HomeAssignment,
        LivenessResult,
        PhysicalRealizationPlan,
    ) {
        let (core, inventory, _, _) = score_plan();
        let level = MinecraftOptimizationLevel::Baseline;
        let demand = crate::lower::minecraft::demand::RuntimeDemand::for_level(
            &core,
            &inventory,
            level,
            crate::lower::minecraft::demand::RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let liveness = LivenessResult::for_level(
            &core,
            &inventory,
            &demand,
            level,
            crate::lower::minecraft::liveness::LivenessLimits::derived(),
        )
        .unwrap();
        let assignment =
            HomeAssignment::for_baseline(&core, &inventory, &demand, &liveness).unwrap();
        let plan = PhysicalRealizationPlan::for_score_compatibility(
            &core,
            &inventory,
            &assignment,
            Some(&liveness),
            PhysicalPlanningLimits::DEFAULT,
        )
        .unwrap();
        (core, inventory, assignment, liveness, plan)
    }

    #[test]
    fn score_compatibility_separates_facts_storage_realizations_and_uses() {
        let (_, _, _, plan) = score_plan();

        assert_eq!(plan.realization_count(), 3);
        assert_eq!(plan.materialization_count(), 2);
        assert!(plan.storage_count() >= 4);
        assert!(plan.requirement_count() >= 6);
        assert_eq!(plan.call_count(), 0);
    }

    #[test]
    fn duplicate_use_requirement_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let requirement_ids = plan
            .requirements
            .iter()
            .map(|(requirement, _)| requirement)
            .take(2)
            .collect::<Vec<_>>();
        let first = requirement_ids[0];
        let second = requirement_ids[1];
        let duplicate = plan.requirements.get(first).unwrap().clone();
        *plan.requirements.get_mut(second).unwrap() = duplicate;
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::IncompleteRequirements { .. })
        ));
    }

    #[test]
    fn corrupted_use_requirement_contract_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let requirement = plan.requirements.keys().next().unwrap();
        let occurrence = plan.requirements.get_mut(requirement).unwrap();
        occurrence.timing = match occurrence.timing {
            ReadTiming::BeforeWrites => ReadTiming::DefinesAtOccurrence,
            ReadTiming::DefinesAtOccurrence => ReadTiming::BeforeWrites,
        };

        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidRequirement { .. })
        ));
    }

    #[test]
    fn duplicate_materialization_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let materialization_ids = plan.materializations.keys().take(2).collect::<Vec<_>>();
        let duplicate = *plan.materializations.get(materialization_ids[0]).unwrap();
        *plan
            .materializations
            .get_mut(materialization_ids[1])
            .unwrap() = duplicate;
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::IncompleteMaterializations { .. })
        ));
    }

    #[test]
    fn corrupted_materialization_recipe_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let materialization = plan.materializations.keys().next().unwrap();
        plan.materializations
            .get_mut(materialization)
            .unwrap()
            .recipe = MaterializationRecipe::ConstantI32ToScore(i32::MAX);

        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidMaterialization { .. })
        ));
    }

    #[test]
    fn detached_realization_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let duplicate = *plan.realizations.iter().next().unwrap().1;
        let detached = plan.realizations.push(duplicate).unwrap();
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::DetachedRealization { realization })
                if realization == detached
        ));
    }

    #[test]
    fn one_value_can_own_several_verified_realization_occurrences() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let declaration = *plan.realizations.iter().next().unwrap().1;
        let additional = plan.realizations.push(declaration).unwrap();
        let owned = &mut plan
            .functions
            .get_mut(declaration.function)
            .unwrap()
            .realizations_by_value[value_index(declaration.value).unwrap()];
        let mut expanded = owned.to_vec();
        expanded.push(additional);
        *owned = expanded.into_boxed_slice();

        plan.verify(
            &core,
            &inventory,
            &assignment,
            None,
            PhysicalPlanningLimits::DEFAULT,
        )
        .unwrap();
    }

    #[test]
    fn corrupted_realization_live_region_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let realization = plan.realizations.keys().next().unwrap();
        plan.realizations.get_mut(realization).unwrap().live_region =
            RealizationLiveRegion::Sparse {
                segment_start: usize::MAX,
                segment_count: usize::MAX,
            };

        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidRealization { .. })
        ));
    }

    #[test]
    fn corrupted_retained_live_segment_is_rejected() {
        let (core, inventory, assignment, liveness, mut plan) = sparse_score_plan();
        plan.live_segments[0].to = plan.live_segments[0].to.saturating_add(1);
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                Some(&liveness),
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidRealization { .. })
        ));
    }

    #[test]
    fn detached_live_segment_is_rejected() {
        let (core, inventory, assignment, liveness, mut plan) = sparse_score_plan();
        let mut segments = plan.live_segments.to_vec();
        segments.push(segments[0]);
        plan.live_segments = segments.into_boxed_slice();
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                Some(&liveness),
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidLiveSegment { .. })
        ));
    }

    #[test]
    fn corrupted_value_fact_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        plan.functions
            .get_mut(FunctionId::from_index(0))
            .unwrap()
            .facts[0] = ValueFact::Unknown;
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidFact { .. })
        ));
    }

    #[test]
    fn corrupted_abi_mode_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        plan.functions
            .get_mut(FunctionId::from_index(0))
            .unwrap()
            .abi
            .results[0]
            .mode = AbiValueMode::ElidedKnown;
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidAbi { .. })
        ));
    }

    #[test]
    fn mistyped_assigned_storage_is_rejected() {
        let (core, inventory, assignment, mut plan) = score_plan();
        let storage = plan
            .storages
            .iter()
            .find_map(|(storage, declaration)| {
                matches!(
                    declaration.binding,
                    PhysicalStorageBinding::AssignedScore { .. }
                )
                .then_some(storage)
            })
            .unwrap();
        let PhysicalStorageBinding::AssignedScore { role, .. } = &mut plan
            .storages
            .get_mut(storage)
            .expect("selected storage exists")
            .binding
        else {
            unreachable!();
        };
        *role = AssignedHomeRole::RecipeTemporary {
            ordinal: usize::MAX,
        };
        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidStorage { storage: invalid }) if invalid == storage
        ));
    }

    fn call_plan() -> (
        CoreProgram,
        SemanticInventory,
        HomeAssignment,
        PhysicalRealizationPlan,
        FunctionId,
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let target = core
            .declare_function(
                Some("callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let entry = core
            .declare_function(
                Some("caller"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut target_builder = FunctionBuilder::new(&core, &sources, target).unwrap();
        let parameter = target_builder
            .body()
            .block(target_builder.entry_block())
            .unwrap()
            .parameters()[0]
            .value();
        target_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(target, target_builder.finish().unwrap())
            .unwrap();
        let mut entry_builder = FunctionBuilder::new(&core, &sources, entry).unwrap();
        let argument = entry_builder.i32_constant(42, OriginId::UNKNOWN).unwrap();
        let result = entry_builder
            .call(target, vec![argument], OriginId::UNKNOWN)
            .unwrap();
        entry_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result[0]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(entry, entry_builder.finish().unwrap())
            .unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let plan = PhysicalRealizationPlan::for_score_compatibility(
            &core,
            &inventory,
            &assignment,
            None,
            PhysicalPlanningLimits::DEFAULT,
        )
        .unwrap();
        (core, inventory, assignment, plan, entry)
    }

    #[test]
    fn call_occurrences_are_complete_and_unique() {
        let (core, inventory, assignment, mut plan, entry) = call_plan();
        assert_eq!(plan.call_count(), 1);
        let duplicate = plan.calls.iter().next().unwrap().1.clone();
        plan.calls.push(duplicate).unwrap();

        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::IncompleteCalls { function }) if function == entry
        ));
    }

    #[test]
    fn corrupted_call_transfer_is_rejected() {
        let (core, inventory, assignment, mut plan, entry) = call_plan();
        let call = plan.calls.keys().next().unwrap();
        let occurrence = plan.calls.get_mut(call).unwrap();
        occurrence.arguments[0].0 = occurrence.results[0].unwrap().1;

        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::InvalidCall { caller, .. }) if caller == entry
        ));
    }

    #[test]
    fn corrupted_call_activation_is_rejected() {
        let (core, inventory, assignment, mut plan, entry) = call_plan();
        let call = plan.calls.keys().next().unwrap();
        plan.calls.get_mut(call).unwrap().activation = ActivationDiscipline::RecursiveStack;

        assert!(matches!(
            plan.verify(
                &core,
                &inventory,
                &assignment,
                None,
                PhysicalPlanningLimits::DEFAULT,
            ),
            Err(PhysicalPlanError::RecursiveSpillLiveness { function, .. })
                if function == entry
        ));
    }

    #[test]
    fn table_admission_is_all_or_nothing() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("main"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let mut limits = PhysicalPlanningLimits::DEFAULT;
        limits.storage_declarations = 0;
        let error = PhysicalRealizationPlan::for_score_compatibility(
            &core,
            &inventory,
            &assignment,
            None,
            limits,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            PhysicalPlanError::Limit {
                table: PhysicalTable::Storages,
                ..
            }
        ));
    }
}
