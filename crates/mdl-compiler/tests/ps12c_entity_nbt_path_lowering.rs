use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

fn pack_text(source: &str) -> String {
    let output = compile_source(SourceInput::new("ps12c.mdl", source), &options()).unwrap();
    output
        .emission()
        .pack()
        .files()
        .iter()
        .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// PS-12C, inline route: a literal index must lower directly to a
/// `data modify ... set from entity ...` command targeting the caller's real
/// result home, with no separate macro helper function and no
/// `mdl:__mdl/macro` frame at all.
#[test]
fn literal_index_lowers_inline_with_no_macro_helper() {
    let source = r#"
export fn read_book() {
    run.as(mc.entities(ArmorStand).with_tag("x").limit(1)) |reader| {
        const page: String = reader.equipment.mainhand.components."minecraft:written_book_content".pages[0].raw;
        if (page.ends_with_ascii("+")) {
            reader.say("MDL_PS12C_OK");
        }
    }
}
"#;
    let pack = pack_text(source);
    assert!(!pack.contains("mdl:__mdl/macro"), "{pack}");
    assert!(!pack.contains("CommandKind::Macro"), "{pack}");

    let read_line = pack
        .lines()
        .find(|line| {
            line.contains("data modify storage")
                && line.contains("set from entity")
                && !line.starts_with('$')
        })
        .expect("inline entity-NBT read line must be present");
    assert!(
        read_line.contains(
            "\"equipment\".\"mainhand\".\"components\".\"minecraft:written_book_content\".\"pages\"[0].\"raw\""
        ),
        "{read_line}"
    );
    let write_target = read_line
        .split("data modify storage ")
        .nth(1)
        .and_then(|rest| rest.split(" set from entity").next())
        .unwrap()
        .trim();
    let empty_init = format!("data modify storage {write_target} set value \"\"");
    assert!(pack.contains(&empty_init), "{pack}");
    // The write target must be a real, independently-referenced result home,
    // not an orphaned scratch path -- proven the same way PS-12.0 proved it.
    let consuming_line = format!("set string storage {write_target}");
    assert!(pack.contains(&consuming_line), "{pack}");
}

/// PS-12C, macro-helper route: a genuinely runtime index must route through
/// the generic crossings.rs engine (an `mdl:__mdl/macro` frame and a
/// `$data modify ... [$(i0)] ...` line), with the read still landing on the
/// caller's real result home -- the exact PS-12.0 regression, now proven
/// through the schema-typed general path instead of the retired intrinsic.
#[test]
fn runtime_index_lowers_through_the_macro_helper_to_the_real_result_home() {
    let source = r#"
export fn read_book() {
    run.as(mc.entities(ArmorStand).with_tag("x").limit(1)) |reader| {
        const index: Int32 = 0;
        const page: String = reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw;
        if (page.ends_with_ascii("+")) {
            reader.say("MDL_PS12C_OK");
        }
    }
}
"#;
    let pack = pack_text(source);
    assert!(pack.contains("mdl:__mdl/macro"), "{pack}");

    let read_line = pack
        .lines()
        .find(|line| line.starts_with('$') && line.contains("set from entity"))
        .expect("macro-rendered entity-NBT read line must be present");
    assert!(read_line.contains("pages[$(i0)]"), "{read_line}");
    assert!(
        !read_line.contains("mdl:preflight"),
        "runtime read still targets the retired scratch placeholder: {read_line}"
    );

    let write_target = read_line
        .split("data modify storage ")
        .nth(1)
        .and_then(|rest| rest.split(" set from entity").next())
        .unwrap()
        .trim();
    let consuming_line = format!("set string storage {write_target}");
    assert!(
        pack.contains(&consuming_line),
        "macro write target {write_target} is never read back by the caller\n{pack}"
    );
}

/// PS-12C, `Bool` inline route: `.resolved` proves the score/scratch
/// conversion path for scalar (non-`String`) schema fields, since
/// `data get` has no entity-source form in this IR yet.
#[test]
fn bool_field_lowers_inline_through_the_scalar_scratch_conversion() {
    let source = r#"
export fn read_resolved() {
    run.as(mc.entities(ArmorStand).with_tag("x").limit(1)) |reader| {
        const resolved: Bool = reader.equipment.mainhand.components."minecraft:written_book_content".resolved;
        if (resolved) {
            reader.say("MDL_PS12C_RESOLVED");
        }
    }
}
"#;
    let pack = pack_text(source);
    assert!(pack.contains("entity_nbt_scalar_scratch"), "{pack}");
    assert!(
        pack.contains(
            "\"equipment\".\"mainhand\".\"components\".\"minecraft:written_book_content\".\"resolved\""
        ),
        "{pack}"
    );
    assert!(pack.contains("execute store result score"), "{pack}");
    assert!(pack.contains("run data get storage"), "{pack}");
}

/// PS-12E's extensibility proof, exercised end to end: `.count` on
/// `ItemStack` was added as a pure schema table-row change (no new
/// checker/HIR/lowering branch), so it must compile and lower through the
/// same generic machinery as the pre-existing fields, on a shorter chain
/// than the book-page family (`equipment.mainhand.count` — no `components`
/// or resource-id step) — the true proof the mechanism generalizes, since
/// the schema-table unit test alone only proves reachability and typing.
#[test]
fn item_count_lowers_inline_through_the_scalar_scratch_conversion() {
    let source = r#"
export fn read_count() {
    run.as(mc.entities(ArmorStand).with_tag("x").limit(1)) |reader| {
        const count: Int32 = reader.equipment.mainhand.count;
        if (count > 1) {
            reader.say("MDL_PS12C_COUNT");
        }
    }
}
"#;
    let pack = pack_text(source);
    assert!(pack.contains("entity_nbt_scalar_scratch"), "{pack}");
    assert!(
        pack.contains("\"equipment\".\"mainhand\".\"count\""),
        "{pack}"
    );
    assert!(pack.contains("execute store result score"), "{pack}");
    assert!(pack.contains("run data get storage"), "{pack}");
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps12c_book").unwrap(),
        ObjectiveName::new("ps12c.book").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::None);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-12C lowering regression"),
    )
}
