use crate::entity::EntityId;
use crate::ir::core::{CoreProgram, FunctionId, TerminatorKind, ValueId};

use super::analysis::{CoreEdgeKind, FunctionSemanticInventory, SemanticInventory};
use super::demand::RuntimeDemand;
use super::plan::{
    BranchArm, BranchEdge, BranchTransfer, EdgeTransfer, FunctionAbi, HomeId, HomeRole, MoveStep,
    PlanBuildError, PlanBuilder, PlannedFunctionRole,
};
use super::transfer::{CopyLocation, ParallelCopyResolver, SymbolicMove};

struct PendingEdge {
    source: crate::ir::core::BlockId,
    kind: CoreEdgeKind,
    moves: Vec<SymbolicMove<HomeId>>,
    helper: Option<super::plan::PlannedFunctionId>,
}

impl PlanBuilder {
    pub(crate) fn physicalize(
        &mut self,
        core: &CoreProgram,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
    ) -> Result<(), PlanBuildError> {
        if !demand.matches_level(self.options.optimization_level()) {
            return Err(PlanBuildError::InvalidRuntimeDemandPolicy);
        }
        // Stage 5E.3 replaces this legacy all-reachable physicalization with the
        // frozen instruction/home assignment that consumes `demand`.
        let _interim_demand_boundary = (demand.completion(), demand.statistics());
        self.initialize_scaffolding()?;
        let mut copy_resolver = ParallelCopyResolver::new();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(PlanBuildError::MissingDefinition { function })?;
            let emitted = inventory
                .function(function)
                .ok_or(PlanBuildError::MissingAnalysis { function })?;
            self.allocate_homes_and_abi(function, declaration, body, emitted)?;
            self.allocate_blocks(function, body, emitted)?;
            let (pending, uses_scratch) =
                self.plan_edges(function, body, emitted, &mut copy_resolver)?;
            let temporary = if uses_scratch {
                Some(self.allocate_home(HomeRole::EdgeTemporary { function })?)
            } else {
                None
            };
            self.functions
                .get_mut(function)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?
                .parallel_copy_temp = temporary;
            self.install_edges(function, body, pending, temporary)?;
        }
        Ok(())
    }

    fn allocate_homes_and_abi(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        emitted: &FunctionSemanticInventory,
    ) -> Result<(), PlanBuildError> {
        for value in emitted.reachable_values().iter().copied() {
            let ty = body
                .value(value)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?
                .ty();
            let home = self.allocate_home(HomeRole::Value {
                function,
                value,
                ty,
            })?;
            let layout = self
                .functions
                .get_mut(function)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?;
            set_slot(&mut layout.value_homes, value, home)?;
        }
        let mut results = Vec::with_capacity(declaration.results().len());
        for (result_index, ty) in declaration.results().iter().copied().enumerate() {
            results.push(self.allocate_home(HomeRole::Result {
                function,
                result_index,
                ty,
            })?);
        }
        let entry = body.entry();
        let parameters = body
            .block(entry)
            .ok_or(PlanBuildError::InvalidCoreEntity { function })?
            .parameters()
            .iter()
            .map(|parameter| self.value_home(function, parameter.value()))
            .collect::<Result<Vec<_>, _>>()?;
        self.functions
            .get_mut(function)
            .ok_or(PlanBuildError::InvalidCoreEntity { function })?
            .abi = Some(FunctionAbi::new(entry, parameters, results));
        Ok(())
    }

    fn allocate_blocks(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        emitted: &FunctionSemanticInventory,
    ) -> Result<(), PlanBuildError> {
        for block in emitted.reachable_blocks().iter().copied() {
            let origin = body
                .block(block)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?
                .origin();
            let planned = self.allocate_planned_function(
                PlannedFunctionRole::Block { function, block },
                origin,
            )?;
            let layout = self
                .functions
                .get_mut(function)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?;
            set_slot(&mut layout.block_functions, block, planned)?;
        }
        Ok(())
    }

    fn plan_edges(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        emitted: &FunctionSemanticInventory,
        copy_resolver: &mut ParallelCopyResolver,
    ) -> Result<(Vec<PendingEdge>, bool), PlanBuildError> {
        let mut pending = Vec::with_capacity(emitted.edges().len());
        let mut uses_scratch = false;
        for edge in emitted.edges() {
            let destination = body
                .block(edge.destination())
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?;
            if destination.parameters().len() != edge.arguments().len() {
                return Err(PlanBuildError::InvalidEdgeShape {
                    function,
                    block: edge.source(),
                });
            }
            let assignments = destination
                .parameters()
                .iter()
                .zip(edge.arguments())
                .map(|(parameter, argument)| {
                    Ok((
                        self.value_home(function, parameter.value())?,
                        self.value_home(function, *argument)?,
                    ))
                })
                .collect::<Result<Vec<_>, PlanBuildError>>()?;
            let moves = copy_resolver
                .resolve(&assignments, self.homes.len())
                .map_err(|_| PlanBuildError::InvalidParallelCopy)?;
            uses_scratch |= moves.iter().any(|step| {
                step.destination == CopyLocation::Scratch || step.source == CopyLocation::Scratch
            });
            let helper = self.allocate_edge_helper(function, body, edge, &moves)?;
            pending.push(PendingEdge {
                source: edge.source(),
                kind: edge.kind(),
                moves,
                helper,
            });
        }
        Ok((pending, uses_scratch))
    }

    fn allocate_edge_helper(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        edge: &super::analysis::CoreEdge,
        moves: &[SymbolicMove<HomeId>],
    ) -> Result<Option<super::plan::PlannedFunctionId>, PlanBuildError> {
        let CoreEdgeKind::Branch(arm) = edge.kind() else {
            return Ok(None);
        };
        if moves.is_empty() {
            return Ok(None);
        }
        let origin = body
            .block(edge.source())
            .and_then(crate::ir::core::BlockData::terminator)
            .map_or(
                crate::source::OriginId::UNKNOWN,
                crate::ir::core::Terminator::origin,
            );
        self.allocate_planned_function(
            PlannedFunctionRole::BranchHelper {
                function,
                edge: BranchEdge::new(edge.source(), arm),
            },
            origin,
        )
        .map(Some)
    }

    fn install_edges(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        pending: Vec<PendingEdge>,
        temporary: Option<HomeId>,
    ) -> Result<(), PlanBuildError> {
        let mut edges = pending.into_iter().peekable();
        while let Some(edge) = edges.next() {
            let transfer = match edge.kind {
                CoreEdgeKind::Jump => EdgeTransfer::Jump {
                    steps: physical_moves(edge.moves, temporary)?.into_boxed_slice(),
                },
                CoreEdgeKind::Branch(BranchArm::Then) => {
                    let else_edge = edges.next().ok_or(PlanBuildError::InvalidEdgeShape {
                        function,
                        block: edge.source,
                    })?;
                    if else_edge.source != edge.source
                        || else_edge.kind != CoreEdgeKind::Branch(BranchArm::Else)
                    {
                        return Err(PlanBuildError::InvalidEdgeShape {
                            function,
                            block: edge.source,
                        });
                    }
                    EdgeTransfer::Branch {
                        then_edge: BranchTransfer::new(
                            physical_moves(edge.moves, temporary)?,
                            edge.helper,
                        ),
                        else_edge: BranchTransfer::new(
                            physical_moves(else_edge.moves, temporary)?,
                            else_edge.helper,
                        ),
                    }
                }
                CoreEdgeKind::Branch(BranchArm::Else) => {
                    return Err(PlanBuildError::InvalidEdgeShape {
                        function,
                        block: edge.source,
                    });
                }
            };
            let terminator = body
                .block(edge.source)
                .and_then(|block| block.terminator())
                .ok_or(PlanBuildError::InvalidEdgeShape {
                    function,
                    block: edge.source,
                })?;
            match (terminator.kind(), &transfer) {
                (TerminatorKind::Jump(_), EdgeTransfer::Jump { .. })
                | (TerminatorKind::Branch { .. }, EdgeTransfer::Branch { .. }) => {}
                _ => {
                    return Err(PlanBuildError::InvalidEdgeShape {
                        function,
                        block: edge.source,
                    });
                }
            }
            set_slot(
                &mut self
                    .functions
                    .get_mut(function)
                    .ok_or(PlanBuildError::InvalidCoreEntity { function })?
                    .edge_transfers,
                edge.source,
                transfer,
            )?;
        }
        Ok(())
    }

    fn value_home(&self, function: FunctionId, value: ValueId) -> Result<HomeId, PlanBuildError> {
        get_slot(
            &self
                .functions
                .get(function)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?
                .value_homes,
            value,
        )
        .ok_or(PlanBuildError::InvalidCoreEntity { function })
    }
}

fn physical_moves(
    moves: Vec<SymbolicMove<HomeId>>,
    temporary: Option<HomeId>,
) -> Result<Vec<MoveStep>, PlanBuildError> {
    moves
        .into_iter()
        .map(|step| {
            Ok(MoveStep::new(
                physical_location(step.destination, temporary)?,
                physical_location(step.source, temporary)?,
            ))
        })
        .collect()
}

fn physical_location(
    location: CopyLocation<HomeId>,
    temporary: Option<HomeId>,
) -> Result<HomeId, PlanBuildError> {
    match location {
        CopyLocation::Home(home) => Ok(home),
        CopyLocation::Scratch => temporary.ok_or(PlanBuildError::InvalidParallelCopy),
    }
}

fn set_slot<I: EntityId, T>(
    slots: &mut [Option<T>],
    id: I,
    value: T,
) -> Result<(), PlanBuildError> {
    let slot = usize::try_from(id.index())
        .ok()
        .and_then(|index| slots.get_mut(index))
        .ok_or(PlanBuildError::InvalidParallelCopy)?;
    if slot.is_some() {
        return Err(PlanBuildError::InvalidParallelCopy);
    }
    *slot = Some(value);
    Ok(())
}

fn get_slot<I: EntityId, T: Copy>(slots: &[Option<T>], id: I) -> Option<T> {
    usize::try_from(id.index())
        .ok()
        .and_then(|index| slots.get(index))
        .copied()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::PlanBuilder;
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind, ValueId,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::plan::EdgeTransfer;
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    struct AllocationFixture {
        program: CoreProgram,
        function: FunctionId,
        entry: BlockId,
        loop_block: BlockId,
        dead: BlockId,
        dead_value: ValueId,
    }

    #[test]
    fn physicalizes_reachable_entities_and_a_genuine_cyclic_backedge() {
        let fixture = allocation_fixture();
        let analyses = SemanticInventory::new(&fixture.program).unwrap();
        let mut builder = PlanBuilder::new(&fixture.program, options()).unwrap();
        builder
            .physicalize(
                &fixture.program,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        let layout = builder.functions.get(fixture.function).unwrap();

        assert!(layout.value_homes[fixture.dead_value.index() as usize].is_none());
        assert!(layout.block_functions[fixture.dead.index() as usize].is_none());
        let temporary = layout.parallel_copy_temp.unwrap();
        let EdgeTransfer::Branch {
            then_edge,
            else_edge,
        } = layout.edge_transfers[fixture.entry.index() as usize]
            .as_ref()
            .unwrap()
        else {
            panic!("entry must have a branch transfer")
        };
        assert!(then_edge.helper().is_some());
        assert!(!then_edge.steps().is_empty());
        assert!(else_edge.helper().is_none());
        assert!(else_edge.steps().is_empty());
        let EdgeTransfer::Jump { steps } = layout.edge_transfers
            [fixture.loop_block.index() as usize]
            .as_ref()
            .unwrap()
        else {
            panic!("loop must have a jump transfer")
        };
        assert_eq!(steps.len(), 3);
        assert!(
            steps
                .iter()
                .any(|step| { step.destination() == temporary || step.source() == temporary })
        );
        assert!(builder.finish(&fixture.program, &analyses).is_ok());
    }

    fn allocation_fixture() -> AllocationFixture {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("loop"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = body.entry_block();
        let first = body.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let second = body.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let condition = body.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let loop_block = body.create_block(OriginId::UNKNOWN).unwrap();
        let loop_first = body
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let loop_second = body
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = body.create_block(OriginId::UNKNOWN).unwrap();
        let dead = body.create_block(OriginId::UNKNOWN).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(loop_block, vec![first, second]),
                else_target: BlockTarget::new(exit, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
        body.switch_to_block(loop_block).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(loop_block, vec![loop_second, loop_first])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
        body.switch_to_block(exit).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
        body.switch_to_block(dead).unwrap();
        let dead_value = body.i32_constant(9, OriginId::UNKNOWN).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
        program
            .define_function(function, body.finish().unwrap())
            .unwrap();
        AllocationFixture {
            program,
            function,
            entry,
            loop_block,
            dead,
            dead_value,
        }
    }

    fn options() -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
    }
}
