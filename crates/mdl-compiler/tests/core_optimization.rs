use mdl_compiler::ir::core::{
    CanonicalPrinter, CoreProgram, CoreType, DebugDumper, FunctionBuilder, FunctionEditor,
    FunctionId, Terminator, TerminatorKind, ValueDef, verify_program,
};
use mdl_compiler::opt::core::{
    CoreOptimizationLevel, CoreOptimizationOptions, CoreOptimizationPhase, CoreOptimizationRemark,
    CorePassLimitReason, CorePipelineStep, CoreRemarkPolicy, OmittedByteCount, StatisticCount,
    optimize_core,
};
use mdl_compiler::source::{Origin, OriginId, SourceContext};

fn empty_return_program(name: &str, sources: &SourceContext, origin: OriginId) -> CoreProgram {
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some(name), vec![], vec![], origin)
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, sources, function).unwrap();
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    program
}

fn undefined_program(name: &str) -> CoreProgram {
    let mut program = CoreProgram::new();
    program
        .declare_function(Some(name), vec![], vec![], OriginId::UNKNOWN)
        .unwrap();
    program
}

fn program_with_detached_history(
    name: &str,
    sources: &SourceContext,
    origin: OriginId,
) -> CoreProgram {
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(Some(name), vec![], vec![], origin)
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, sources, function).unwrap();
    let unused = builder.i32_constant(7, origin).unwrap();
    let ValueDef::InstResult { instruction, .. } =
        builder.body().value(unused).unwrap().definition()
    else {
        panic!("constant must be an instruction result");
    };
    builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
        .unwrap();
    let mut body = builder.finish().unwrap();
    FunctionEditor::new(&program, sources, function, &mut body)
        .unwrap()
        .erase_pure_inst(instruction)
        .unwrap();
    program.define_function(function, body).unwrap();
    program
}

fn define_cse_remark_function(
    program: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
) {
    let mut builder = FunctionBuilder::new(program, sources, function).unwrap();
    let entry = builder.entry_block();
    let input = builder.body().block(entry).unwrap().parameters()[0].value();
    let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
    let representative = builder
        .i32_add_wrapping(input, one, OriginId::UNKNOWN)
        .unwrap();
    let first_duplicate = builder
        .i32_add_wrapping(input, one, OriginId::UNKNOWN)
        .unwrap();
    let second_duplicate = builder
        .i32_add_wrapping(input, one, OriginId::UNKNOWN)
        .unwrap();
    let partial = builder
        .i32_add_wrapping(representative, first_duplicate, OriginId::UNKNOWN)
        .unwrap();
    let result = builder
        .i32_add_wrapping(partial, second_duplicate, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
}

#[test]
fn none_preserves_program_text_and_backing_storage_with_a_stable_report() {
    let sources = SourceContext::new();
    let program = program_with_detached_history("entry", &sources, OriginId::UNKNOWN);
    let function = program.functions().next().unwrap().0;
    let allocation_before = std::ptr::from_ref(program.function(function).unwrap());
    let canonical_before = CanonicalPrinter::new(&program, &sources).unwrap().render();
    let debug_before = DebugDumper::new(&program).render();
    assert!(debug_before.contains("<detached>"));
    let options = CoreOptimizationOptions::new(CoreOptimizationLevel::None);

    let output = optimize_core(program, &sources, &options).unwrap();

    assert_eq!(output.report().level(), CoreOptimizationLevel::None);
    assert!(output.report().remarks().is_none());
    let first_dump = output.report().dump();
    assert_eq!(first_dump, "core-optimization level=none\n");
    assert_eq!(output.report().dump(), first_dump);
    let (program, report) = output.into_parts();
    assert_eq!(
        std::ptr::from_ref(program.function(function).unwrap()),
        allocation_before,
        "representation regression: None should preserve the owned function store"
    );
    assert_eq!(
        CanonicalPrinter::new(&program, &sources).unwrap().render(),
        canonical_before
    );
    assert_eq!(DebugDumper::new(&program).render(), debug_before);
    verify_program(&program, &sources).unwrap();
    assert_eq!(report.dump(), "core-optimization level=none\n");
}

#[test]
fn snapshot_options_apply_a_hard_smart_cap() {
    let options = CoreOptimizationOptions::default().with_failure_snapshot_byte_limit(usize::MAX);

    assert_eq!(options.level(), CoreOptimizationLevel::None);
    assert_eq!(
        options.failure_snapshot_byte_limit(),
        CoreOptimizationOptions::MAX_FAILURE_SNAPSHOT_BYTE_LIMIT
    );
}

#[test]
fn an_input_larger_than_the_hard_cap_retains_only_the_capped_prefix() {
    let sources = SourceContext::new();
    let name = "x".repeat(CoreOptimizationOptions::MAX_FAILURE_SNAPSHOT_BYTE_LIMIT + 257);
    let program = undefined_program(&name);
    let complete = DebugDumper::new(&program).render();

    let failure = optimize_core(
        program,
        &sources,
        &CoreOptimizationOptions::default().with_failure_snapshot_byte_limit(usize::MAX),
    )
    .unwrap_err();

    let retained = CoreOptimizationOptions::MAX_FAILURE_SNAPSHOT_BYTE_LIMIT;
    assert_eq!(failure.snapshot().retained_bytes(), retained);
    assert_eq!(failure.snapshot().text(), &complete[..retained]);
    assert_eq!(
        failure.snapshot().omitted_bytes(),
        OmittedByteCount::Exact(u64::try_from(complete.len() - retained).unwrap())
    );
}

#[test]
fn zero_and_exact_caps_report_exact_omitted_bytes() {
    let sources = SourceContext::new();
    let program = undefined_program("undefined");
    let complete = DebugDumper::new(&program).render();

    let zero = optimize_core(
        program.clone(),
        &sources,
        &CoreOptimizationOptions::default().with_failure_snapshot_byte_limit(0),
    )
    .unwrap_err();
    assert_eq!(zero.phase(), CoreOptimizationPhase::InputVerification);
    assert!(zero.diagnostics().contains_code("core.undefined-function"));
    assert_eq!(zero.snapshot().text(), "");
    assert_eq!(zero.snapshot().retained_bytes(), 0);
    assert_eq!(
        zero.snapshot().omitted_bytes(),
        OmittedByteCount::Exact(u64::try_from(complete.len()).unwrap())
    );
    assert!(zero.snapshot().is_truncated());

    let exact = optimize_core(
        program,
        &sources,
        &CoreOptimizationOptions::default().with_failure_snapshot_byte_limit(complete.len()),
    )
    .unwrap_err();
    assert_eq!(exact.snapshot().text(), complete);
    assert_eq!(exact.snapshot().omitted_bytes(), OmittedByteCount::Exact(0));
    assert!(!exact.snapshot().is_truncated());
}

#[test]
fn every_cap_retains_the_longest_valid_utf8_prefix() {
    let sources = SourceContext::new();
    let program = undefined_program("π_target_🦀_雪");
    let complete = DebugDumper::new(&program).render();

    for cap in 0..=complete.len() {
        let failure = optimize_core(
            program.clone(),
            &sources,
            &CoreOptimizationOptions::default().with_failure_snapshot_byte_limit(cap),
        )
        .unwrap_err();
        let mut expected_end = cap;
        while !complete.is_char_boundary(expected_end) {
            expected_end -= 1;
        }
        assert_eq!(failure.snapshot().text(), &complete[..expected_end]);
        assert_eq!(failure.snapshot().retained_bytes(), expected_end);
        assert_eq!(
            failure.snapshot().omitted_bytes(),
            OmittedByteCount::Exact(u64::try_from(complete.len() - expected_end).unwrap())
        );
    }
}

#[test]
fn repeated_invalid_inputs_produce_identical_failures() {
    let sources = SourceContext::new();
    let program = undefined_program("repeat");
    let options = CoreOptimizationOptions::default().with_failure_snapshot_byte_limit(17);

    let first = optimize_core(program.clone(), &sources, &options).unwrap_err();
    let second = optimize_core(program, &sources, &options).unwrap_err();

    assert_eq!(first, second);
    assert_eq!(first.to_string(), second.to_string());
    let (phase, diagnostics, snapshot) = first.into_parts();
    assert_eq!(phase, CoreOptimizationPhase::InputVerification);
    assert!(diagnostics.contains_code("core.undefined-function"));
    let (text, omitted) = snapshot.into_parts();
    assert!(text.len() <= 17);
    assert!(matches!(omitted, OmittedByteCount::Exact(_)));
}

#[test]
fn detached_source_context_is_a_typed_input_verification_failure() {
    let mut construction_sources = SourceContext::new();
    let detached_origin = construction_sources.add_origin(Origin::Unknown).unwrap();
    let program = empty_return_program("detached", &construction_sources, detached_origin);

    let failure = optimize_core(
        program,
        &SourceContext::new(),
        &CoreOptimizationOptions::default(),
    )
    .unwrap_err();

    assert_eq!(failure.phase(), CoreOptimizationPhase::InputVerification);
    assert!(failure.diagnostics().contains_code("core.invalid-origin"));
    assert!(failure.snapshot().text().contains("function @fn0"));
}

#[test]
fn public_remark_policy_filters_caps_and_exposes_real_cse_correspondence() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let ignored = program
        .declare_function(
            Some("ignored"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let selected = program
        .declare_function(
            Some("selected"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    define_cse_remark_function(&mut program, &sources, ignored);
    define_cse_remark_function(&mut program, &sources, selected);

    let policy = CoreRemarkPolicy::cse_applied(1).with_function_filter(selected);
    let options =
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline).with_remark_policy(policy);
    assert_eq!(options.remark_policy(), policy);

    let output = optimize_core(program, &sources, &options).unwrap();
    let remarks = output
        .report()
        .remarks()
        .expect("an explicitly enabled policy returns its bounded stream");
    assert_eq!(remarks.cse_applied_count(), StatisticCount::Exact(2));
    assert_eq!(remarks.omitted_count(), StatisticCount::Exact(1));
    assert_eq!(remarks.records().len(), 1);
    assert_eq!(
        remarks.limit_miss_count(CorePassLimitReason::CseRootVisits),
        StatisticCount::Exact(0)
    );
    match &remarks.records()[0] {
        CoreOptimizationRemark::CseApplied {
            function,
            step,
            duplicate,
            representative,
            duplicate_results,
            representative_results,
            ..
        } => {
            assert_eq!(*function, selected);
            assert_eq!(*step, CorePipelineStep::Cse);
            assert_ne!(duplicate, representative);
            assert_eq!(duplicate_results.len(), 1);
            assert_eq!(representative_results.len(), 1);
        }
        _ => panic!("expected one retained applied-CSE record"),
    }
}
