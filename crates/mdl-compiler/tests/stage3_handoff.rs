use mdl_compiler::datapack::{EmissionOptions, emit_datapack};
use mdl_compiler::ir::minecraft::{
    CommandKind, CommandNode, Condition, ExecuteCommand, ExecuteModifier, ExecuteModifierKind,
    ExecuteModifiers, FakeScoreHolder, FunctionCall, FunctionResourceId, InternalCallableRef,
    McFunctionId, MinecraftProgramBuilder, ObjectiveName, ReturnCommand, ScoreRange, ScoreRef,
    SingleScoreHolder, UnsafeRawCommand,
};
use mdl_compiler::source::{OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;

#[test]
fn public_stage3_api_expresses_stage4_branch_and_call_handoffs_exactly() {
    let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
    let when_true = declare(&mut builder, "mdl:true");
    let when_false = declare(&mut builder, "mdl:false");
    let gate = declare(&mut builder, "mdl:gate");
    let dual_guard = declare(&mut builder, "mdl:dual_guard");
    let dispatcher = declare(&mut builder, "mdl:dispatcher");
    let ordinary = declare(&mut builder, "mdl:ordinary");
    let tail = declare(&mut builder, "mdl:tail");
    let function_condition = declare(&mut builder, "mdl:function_condition");

    define(&mut builder, when_true, vec![raw("say TRUE")]);
    define(&mut builder, when_false, vec![raw("say FALSE")]);
    define(&mut builder, gate, vec![guarded_call(true, when_true)]);
    define(
        &mut builder,
        dual_guard,
        vec![
            guarded_call(true, when_true),
            guarded_call(false, when_false),
        ],
    );
    define(
        &mut builder,
        dispatcher,
        vec![
            execute(
                true,
                node(CommandKind::Return(ReturnCommand::run(call(when_true)))),
            ),
            node(CommandKind::Return(ReturnCommand::run(call(when_false)))),
        ],
    );
    define(
        &mut builder,
        ordinary,
        vec![call(when_true), raw("say RESUMED")],
    );
    define(
        &mut builder,
        tail,
        vec![node(CommandKind::Return(ReturnCommand::run(call(
            when_true,
        ))))],
    );
    define(
        &mut builder,
        function_condition,
        vec![execute_with_condition(
            true,
            Condition::Function(when_true),
            call(when_false),
        )],
    );

    let program = builder.finish().unwrap();
    let output = emit_datapack(
        &program,
        &SourceContext::new(),
        &EmissionOptions::new("Stage 4 handoff"),
    )
    .unwrap();
    assert_eq!(output.pack().files().len(), 9);
    assert_function(
        &output,
        "gate",
        "execute if score #condition mdl.reg matches 1 run function mdl:true\n",
    );
    assert_function(
        &output,
        "dual_guard",
        concat!(
            "execute if score #condition mdl.reg matches 1 run function mdl:true\n",
            "execute unless score #condition mdl.reg matches 1 run function mdl:false\n",
        ),
    );
    assert_function(
        &output,
        "dispatcher",
        concat!(
            "execute if score #condition mdl.reg matches 1 run return run function mdl:true\n",
            "return run function mdl:false\n",
        ),
    );
    assert_function(&output, "ordinary", "function mdl:true\nsay RESUMED\n");
    assert_function(&output, "tail", "return run function mdl:true\n");
    assert_function(
        &output,
        "function_condition",
        "execute if function mdl:true run function mdl:false\n",
    );
}

fn declare(builder: &mut MinecraftProgramBuilder, resource: &str) -> McFunctionId {
    builder
        .declare_function(
            FunctionResourceId::parse(resource).unwrap(),
            OriginId::UNKNOWN,
        )
        .unwrap()
}

fn define(
    builder: &mut MinecraftProgramBuilder,
    function: McFunctionId,
    commands: Vec<CommandNode>,
) {
    let mut body = builder.begin_function(function).unwrap();
    for command in commands {
        body.push(command).unwrap();
    }
    body.finish();
}

fn condition() -> Condition {
    Condition::ScoreMatches(
        ScoreRef::new(
            SingleScoreHolder::from(FakeScoreHolder::new("#condition").unwrap()),
            ObjectiveName::new("mdl.reg").unwrap(),
        ),
        ScoreRange::exact(1),
    )
}

fn call(function: McFunctionId) -> CommandNode {
    node(CommandKind::Function(FunctionCall::new(
        InternalCallableRef::Function(function).into(),
    )))
}

fn guarded_call(positive: bool, function: McFunctionId) -> CommandNode {
    execute(positive, call(function))
}

fn execute(positive: bool, run: CommandNode) -> CommandNode {
    execute_with_condition(positive, condition(), run)
}

fn execute_with_condition(positive: bool, condition: Condition, run: CommandNode) -> CommandNode {
    let modifier = if positive {
        ExecuteModifierKind::If(condition)
    } else {
        ExecuteModifierKind::Unless(condition)
    };
    node(CommandKind::Execute(ExecuteCommand::new(
        ExecuteModifiers::new(ExecuteModifier::new(modifier, OriginId::UNKNOWN), vec![]),
        run,
    )))
}

fn raw(line: &str) -> CommandNode {
    node(CommandKind::Raw(UnsafeRawCommand::new(line).unwrap()))
}

fn node(kind: CommandKind) -> CommandNode {
    CommandNode::new(kind, OriginId::UNKNOWN).unwrap()
}

fn assert_function(output: &mdl_compiler::datapack::EmissionOutput, name: &str, expected: &str) {
    let path = format!("data/mdl/function/{name}.mcfunction");
    assert_eq!(
        output.pack().file(&path).unwrap().bytes(),
        expected.as_bytes(),
        "{path}"
    );
}
