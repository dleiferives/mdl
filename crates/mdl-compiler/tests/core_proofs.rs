use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CanonicalPrinter, ControlFlowGraph, CoreProgram, CoreType, DebugDumper,
    EditError, FunctionBuilder, FunctionEditor, I32Predicate, PlacementIndex, Terminator,
    TerminatorKind, UseIndex, ValueDef, ValueId, verify_function, verify_program,
};
use mdl_compiler::source::{Origin, OriginId, SourceContext};

const UNKNOWN: OriginId = OriginId::UNKNOWN;

fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
    builder.body().block(block).unwrap().parameters()[index].value()
}

#[test]
fn canonical_value_producing_diamond_is_stable() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("choose"),
            vec![CoreType::Bool, CoreType::I32, CoreType::I32],
            vec![CoreType::I32],
            UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let condition = parameter(&builder, entry, 0);
    let left = parameter(&builder, entry, 1);
    let right = parameter(&builder, entry, 2);
    let then_block = builder.create_block(UNKNOWN).unwrap();
    let else_block = builder.create_block(UNKNOWN).unwrap();
    let join = builder.create_block(UNKNOWN).unwrap();
    let result = builder
        .append_block_parameter(join, CoreType::I32, UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(then_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![left])),
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(else_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![right])),
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(join).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();

    let printed = CanonicalPrinter::new(&program, &sources).unwrap().render();
    assert_eq!(
        printed,
        concat!(
            "func @fn0(%v0: bool, %v1: i32, %v2: i32) -> (i32) // choose {\n",
            "^bb0:\n",
            "  branch %v0, ^bb1, ^bb2\n",
            "^bb1():\n",
            "  jump ^bb3(%v1)\n",
            "^bb2():\n",
            "  jump ^bb3(%v2)\n",
            "^bb3(%v3: i32):\n",
            "  return %v3\n",
            "}\n",
        )
    );
    assert_eq!(
        printed,
        CanonicalPrinter::new(&program, &sources).unwrap().render()
    );
}

#[test]
fn verifies_nested_boolean_branches() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("nested"),
            vec![CoreType::Bool, CoreType::Bool],
            vec![],
            UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let first = parameter(&builder, entry, 0);
    let second = parameter(&builder, entry, 1);
    let nested = builder.create_block(UNKNOWN).unwrap();
    let yes = builder.create_block(UNKNOWN).unwrap();
    let no = builder.create_block(UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: first,
                then_target: BlockTarget::new(nested, vec![]),
                else_target: BlockTarget::new(no, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(nested).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: second,
                then_target: BlockTarget::new(yes, vec![]),
                else_target: BlockTarget::new(no, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    for block in [yes, no] {
        builder.switch_to_block(block).unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
            .unwrap();
    }
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    verify_program(&program, &sources).unwrap();
}

#[test]
fn verifies_loop_carried_counter_and_accumulator() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("sum_down"),
            vec![CoreType::I32, CoreType::I32],
            vec![CoreType::I32],
            UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let initial_counter = parameter(&builder, entry, 0);
    let initial_accumulator = parameter(&builder, entry, 1);
    let header = builder.create_block(UNKNOWN).unwrap();
    let counter = builder
        .append_block_parameter(header, CoreType::I32, UNKNOWN)
        .unwrap();
    let accumulator = builder
        .append_block_parameter(header, CoreType::I32, UNKNOWN)
        .unwrap();
    let loop_body = builder.create_block(UNKNOWN).unwrap();
    let exit = builder.create_block(UNKNOWN).unwrap();
    let result = builder
        .append_block_parameter(exit, CoreType::I32, UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![initial_counter, initial_accumulator],
            )),
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(header).unwrap();
    let zero = builder.i32_constant(0, UNKNOWN).unwrap();
    let done = builder
        .i32_compare(I32Predicate::SignedLe, counter, zero, UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: done,
                then_target: BlockTarget::new(exit, vec![accumulator]),
                else_target: BlockTarget::new(loop_body, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(loop_body).unwrap();
    let minus_one = builder.i32_constant(-1, UNKNOWN).unwrap();
    let next_counter = builder
        .i32_add_wrapping(counter, minus_one, UNKNOWN)
        .unwrap();
    let next_accumulator = builder
        .i32_add_wrapping(accumulator, counter, UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![next_counter, next_accumulator],
            )),
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(exit).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    verify_program(&program, &sources).unwrap();
}

#[test]
fn overflowing_add_has_two_typed_results() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("overflow"),
            vec![CoreType::I32, CoreType::I32],
            vec![CoreType::I32, CoreType::Bool],
            UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let left = parameter(&builder, entry, 0);
    let right = parameter(&builder, entry, 1);
    let (sum, overflowed) = builder.i32_add_overflowing(left, right, UNKNOWN).unwrap();
    assert_eq!(builder.body().value(sum).unwrap().ty(), CoreType::I32);
    assert_eq!(
        builder.body().value(overflowed).unwrap().ty(),
        CoreType::Bool
    );
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![sum, overflowed]),
            UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    verify_program(&program, &sources).unwrap();
}

#[test]
fn forward_recursive_and_mutually_recursive_calls_verify() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let first = program
        .declare_function(
            Some("first"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            UNKNOWN,
        )
        .unwrap();
    let second = program
        .declare_function(
            Some("second"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            UNKNOWN,
        )
        .unwrap();

    for (function, callee) in [(first, second), (second, first)] {
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let argument = parameter(&builder, entry, 0);
        let result = builder.call(callee, vec![argument], UNKNOWN).unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        program.define_function(function, body).unwrap();
    }
    verify_program(&program, &sources).unwrap();
    let text = CanonicalPrinter::new(&program, &sources).unwrap().render();
    assert!(text.contains("core.call @fn1"));
    assert!(text.contains("core.call @fn0"));
}

#[test]
fn invalid_reachable_cross_branch_use_is_rejected() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("bad"),
            vec![CoreType::Bool],
            vec![CoreType::I32],
            UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let condition = parameter(&builder, entry, 0);
    let then_block = builder.create_block(UNKNOWN).unwrap();
    let else_block = builder.create_block(UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(then_block).unwrap();
    let leaked = builder.i32_constant(7, UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![leaked]),
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(else_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![leaked]),
            UNKNOWN,
        ))
        .unwrap();

    let diagnostics = builder.finish().unwrap_err();
    assert!(diagnostics.contains_code("core.dominance"));
}

#[test]
fn branch_folding_then_unreachable_cleanup_preserves_validity() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some("fold"), vec![CoreType::Bool], vec![], UNKNOWN)
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let condition = parameter(&builder, entry, 0);
    let live = builder.create_block(UNKNOWN).unwrap();
    let dead = builder.create_block(UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(live, vec![]),
                else_target: BlockTarget::new(dead, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(live).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
        .unwrap();
    builder.switch_to_block(dead).unwrap();
    let dead_value = builder.i32_constant(99, UNKNOWN).unwrap();
    let dead_instruction = match builder.body().value(dead_value).unwrap().definition() {
        ValueDef::InstResult { instruction, .. } => instruction,
        ValueDef::BlockParam { .. } => unreachable!(),
    };
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
        .unwrap();
    let mut body = builder.finish().unwrap();
    {
        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
        editor
            .set_terminator(
                entry,
                Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(live, vec![])),
                    UNKNOWN,
                ),
            )
            .unwrap();
        assert_eq!(editor.detach_unreachable_blocks().unwrap(), 1);
    }
    assert_eq!(body.block_counts().allocated, 3);
    assert_eq!(body.block_counts().attached, 2);
    let placement = PlacementIndex::new(&body);
    assert!(!placement.is_block_attached(dead));
    assert!(!placement.is_instruction_attached(dead_instruction));
    let cfg = ControlFlowGraph::new(&body);
    assert!(!cfg.is_attached(dead));
    let uses = UseIndex::new(&body);
    assert!(uses.uses(dead_value).is_empty());
    drop(uses);
    drop(cfg);
    drop(placement);
    program.define_function(function, body).unwrap();
    verify_program(&program, &sources).unwrap();
    assert!(DebugDumper::new(&program).render().contains("<detached>"));
}

#[test]
fn source_call_site_and_fused_origins_are_accepted() {
    let mut sources = SourceContext::new();
    let file = sources.add_file("origins.mdl", "call()").unwrap();
    let span = sources.span(file, 0, 6).unwrap();
    let source = sources.add_origin(Origin::Source(span)).unwrap();
    let call_site = sources
        .add_origin(Origin::CallSite {
            callee: source,
            caller: source,
        })
        .unwrap();
    let fused = sources
        .add_origin(Origin::Fused {
            inputs: vec![source, call_site],
            reason: Some("lowered".into()),
        })
        .unwrap();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some("origins"), vec![], vec![], fused)
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), call_site))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    verify_program(&program, &sources).unwrap();
}

#[test]
fn effectful_unused_call_cannot_be_erased() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let target_function = program
        .declare_function(Some("callee"), vec![], vec![], UNKNOWN)
        .unwrap();
    let invoking_function = program
        .declare_function(Some("caller"), vec![], vec![], UNKNOWN)
        .unwrap();
    let mut target_builder = FunctionBuilder::new(&program, &sources, target_function).unwrap();
    target_builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
        .unwrap();
    program
        .define_function(target_function, target_builder.finish().unwrap())
        .unwrap();

    let mut invoking_builder = FunctionBuilder::new(&program, &sources, invoking_function).unwrap();
    invoking_builder
        .call(target_function, vec![], UNKNOWN)
        .unwrap();
    let call_inst = invoking_builder
        .body()
        .block(invoking_builder.entry_block())
        .unwrap()
        .instructions()[0];
    invoking_builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
        .unwrap();
    let mut body = invoking_builder.finish().unwrap();
    let before = format!("{body:#?}");
    let error = FunctionEditor::new(&program, &sources, invoking_function, &mut body)
        .unwrap()
        .erase_pure_inst(call_inst)
        .unwrap_err();
    assert_eq!(
        error,
        EditError::NotPure {
            instruction: call_inst
        }
    );
    assert_eq!(format!("{body:#?}"), before);
}

#[test]
fn rejected_dominance_replacement_leaves_body_identical() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("replace"),
            vec![CoreType::Bool, CoreType::I32],
            vec![CoreType::I32],
            UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let entry = builder.entry_block();
    let condition = parameter(&builder, entry, 0);
    let old = parameter(&builder, entry, 1);
    let then_block = builder.create_block(UNKNOWN).unwrap();
    let else_block = builder.create_block(UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(then_block).unwrap();
    let replacement = builder.i32_constant(9, UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![old]), UNKNOWN))
        .unwrap();
    builder.switch_to_block(else_block).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![old]), UNKNOWN))
        .unwrap();
    let mut body = builder.finish().unwrap();
    let before = format!("{body:#?}");
    let error = FunctionEditor::new(&program, &sources, function, &mut body)
        .unwrap()
        .replace_value(old, replacement)
        .unwrap_err();
    assert!(matches!(error, EditError::DoesNotDominate { .. }));
    assert_eq!(format!("{body:#?}"), before);
}

#[test]
fn replacement_rewrites_instruction_edge_and_return_uses() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some("all_uses"), vec![], vec![CoreType::I32], UNKNOWN)
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let old = builder.i32_constant(1, UNKNOWN).unwrap();
    let new = builder.i32_constant(2, UNKNOWN).unwrap();
    let ordinary = builder.i32_add_wrapping(old, old, UNKNOWN).unwrap();
    let join = builder.create_block(UNKNOWN).unwrap();
    let joined = builder
        .append_block_parameter(join, CoreType::I32, UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![old])),
            UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(join).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![joined]),
            UNKNOWN,
        ))
        .unwrap();
    let mut body = builder.finish().unwrap();
    FunctionEditor::new(&program, &sources, function, &mut body)
        .unwrap()
        .replace_value(old, new)
        .unwrap();

    let ordinary_inst = match body.value(ordinary).unwrap().definition() {
        ValueDef::InstResult { instruction, .. } => instruction,
        ValueDef::BlockParam { .. } => unreachable!(),
    };
    assert_eq!(
        body.instruction(ordinary_inst).unwrap().operands(),
        &[new, new]
    );
    let entry = body.block(body.entry()).unwrap();
    let TerminatorKind::Jump(target) = entry.terminator().unwrap().kind() else {
        panic!("expected jump");
    };
    assert_eq!(target.arguments(), &[new]);
}

#[test]
fn pure_instruction_replacement_rewrites_results_and_detaches_old_instruction() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some("replace_inst"), vec![], vec![CoreType::I32], UNKNOWN)
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    let old_value = builder.i32_constant(1, UNKNOWN).unwrap();
    let new_value = builder.i32_constant(2, UNKNOWN).unwrap();
    let old_inst = match builder.body().value(old_value).unwrap().definition() {
        ValueDef::InstResult { instruction, .. } => instruction,
        ValueDef::BlockParam { .. } => unreachable!(),
    };
    let new_inst = match builder.body().value(new_value).unwrap().definition() {
        ValueDef::InstResult { instruction, .. } => instruction,
        ValueDef::BlockParam { .. } => unreachable!(),
    };
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![old_value]),
            UNKNOWN,
        ))
        .unwrap();
    let mut body = builder.finish().unwrap();
    FunctionEditor::new(&program, &sources, function, &mut body)
        .unwrap()
        .replace_pure_inst(old_inst, new_inst)
        .unwrap();
    assert!(!PlacementIndex::new(&body).is_instruction_attached(old_inst));
    let TerminatorKind::Return(values) = body
        .block(body.entry())
        .unwrap()
        .terminator()
        .unwrap()
        .kind()
    else {
        panic!("expected return");
    };
    assert_eq!(values, &[new_value]);
    verify_function(&program, &sources, function, &body).unwrap();
}

#[test]
fn unknown_effect_calls_retain_source_order() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let first_target = program
        .declare_function(Some("first_target"), vec![], vec![], UNKNOWN)
        .unwrap();
    let second_target = program
        .declare_function(Some("second_target"), vec![], vec![], UNKNOWN)
        .unwrap();
    let driver = program
        .declare_function(Some("driver"), vec![], vec![], UNKNOWN)
        .unwrap();
    for target in [first_target, second_target] {
        let mut builder = FunctionBuilder::new(&program, &sources, target).unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
            .unwrap();
        let body = builder.finish().unwrap();
        program.define_function(target, body).unwrap();
    }
    let mut builder = FunctionBuilder::new(&program, &sources, driver).unwrap();
    builder.call(first_target, vec![], UNKNOWN).unwrap();
    builder.call(second_target, vec![], UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
        .unwrap();
    program
        .define_function(driver, builder.finish().unwrap())
        .unwrap();

    let text = CanonicalPrinter::new(&program, &sources).unwrap().render();
    let first_position = text.find("core.call @fn0").unwrap();
    let second_position = text.find("core.call @fn1").unwrap();
    assert!(first_position < second_position);
}

#[test]
fn declaration_lifecycle_rejects_missing_and_duplicate_definitions() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some("lifecycle"), vec![], vec![], UNKNOWN)
        .unwrap();
    assert!(
        verify_program(&program, &sources)
            .unwrap_err()
            .contains_code("core.undefined-function")
    );

    let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), UNKNOWN))
        .unwrap();
    let body = builder.finish().unwrap();
    program.define_function(function, body.clone()).unwrap();
    assert!(program.define_function(function, body).is_err());
    verify_program(&program, &sources).unwrap();
}

#[test]
fn generated_edit_sequences_preserve_invariants_or_reject_without_mutation() {
    const GENERATOR_VERSION: u32 = 1;

    for seed in 0_u8..16 {
        let operation = match seed % 4 {
            0 => "replace-first-with-second",
            1 => "replace-first-with-sum",
            2 => "erase-used-first",
            _ => "erase-unused-second",
        };
        let first_literal = i32::from(seed);
        let second_literal = first_literal + 1;
        let context = format!(
            "generator_version={GENERATOR_VERSION} seed={seed} shape=linear-i32-edit inputs=first:{first_literal},second:{second_literal},sum:first+first,operation:{operation}"
        );
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("generated"), vec![], vec![CoreType::I32], UNKNOWN)
            .unwrap_or_else(|error| panic!("{context}: declaration failed: {error:?}"));
        let mut builder =
            FunctionBuilder::new(&program, &sources, function).unwrap_or_else(|diagnostics| {
                panic!("{context}: builder creation failed: {diagnostics:?}")
            });
        let first = builder
            .i32_constant(first_literal, UNKNOWN)
            .unwrap_or_else(|error| panic!("{context}: first constant failed: {error:?}"));
        let second = builder
            .i32_constant(second_literal, UNKNOWN)
            .unwrap_or_else(|error| panic!("{context}: second constant failed: {error:?}"));
        let sum = builder
            .i32_add_wrapping(first, first, UNKNOWN)
            .unwrap_or_else(|error| panic!("{context}: sum creation failed: {error:?}"));
        let first_inst = match builder
            .body()
            .value(first)
            .unwrap_or_else(|| panic!("{context}: first value is missing"))
            .definition()
        {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => {
                unreachable!("{context}: first constant became a block parameter")
            }
        };
        let second_inst = match builder
            .body()
            .value(second)
            .unwrap_or_else(|| panic!("{context}: second value is missing"))
            .definition()
        {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => {
                unreachable!("{context}: second constant became a block parameter")
            }
        };
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![sum]), UNKNOWN))
            .unwrap_or_else(|error| panic!("{context}: return creation failed: {error:?}"));
        let mut body = builder.finish().unwrap_or_else(|diagnostics| {
            panic!("{context}: generated body is invalid: {diagnostics:?}")
        });
        let before = format!("{body:#?}");
        let result = {
            let mut editor = FunctionEditor::new(&program, &sources, function, &mut body)
                .unwrap_or_else(|diagnostics| {
                    panic!("{context}: editor creation failed: {diagnostics:?}")
                });
            match seed % 4 {
                0 => editor.replace_value(first, second),
                1 => editor.replace_value(first, sum),
                2 => editor.erase_pure_inst(first_inst),
                _ => editor.erase_pure_inst(second_inst),
            }
        };
        match result {
            Ok(()) => {
                verify_function(&program, &sources, function, &body).unwrap_or_else(
                    |diagnostics| {
                        panic!("{context}: accepted edit produced invalid IR: {diagnostics:?}")
                    },
                );
            }
            Err(error) => assert_eq!(
                format!("{body:#?}"),
                before,
                "{context}: rejected edit {error:?} mutated the body"
            ),
        }
    }
}
