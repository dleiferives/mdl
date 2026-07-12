use std::collections::HashSet;

use serde::Serialize;

use crate::datapack::{DatapackArtifact, EmissionOutput, FunctionTrace, PackFile, TraceMap};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityVec;
use crate::ir::minecraft::{
    ExternalCallableRef, ExternalTagRequirement, FunctionTagEntryKind, FunctionTagMerge,
    InternalCallableRef, MinecraftProgram, PackPath, RenderError, render_function, verify_program,
};
use crate::source::{OriginId, SourceContext};

/// User-controlled, nonsemantic datapack emission options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmissionOptions {
    description: Box<str>,
}

impl EmissionOptions {
    /// Creates output options with a plain-text pack description.
    #[must_use]
    pub fn new(description: impl Into<Box<str>>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Returns the plain-text pack description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Verifies and deterministically emits one complete in-memory datapack.
///
/// # Errors
///
/// Returns structural verification, target rendering, or output serialization
/// diagnostics. No partial artifact is returned.
pub fn emit_datapack(
    program: &MinecraftProgram,
    sources: &SourceContext,
    options: &EmissionOptions,
) -> Result<EmissionOutput, Diagnostics> {
    verify_program(program, sources)?;

    let mut findings = vec![];
    let mut files = vec![];
    match serialize_metadata(program, options) {
        Ok(bytes) => files.push(PackFile::new(PackPath::metadata(), bytes)),
        Err(error) => findings.push(Diagnostic::new(
            "datapack.metadata-json",
            format!("failed to serialize pack metadata: {error}"),
            OriginId::UNKNOWN,
        )),
    }

    let mut trace_values = Vec::with_capacity(program.functions().len());
    for (_, function) in program.functions() {
        match render_function(program, function) {
            Ok(bytes) => {
                files.push(PackFile::new(
                    function.resource().pack_path(program.target()),
                    bytes,
                ));
                let origins = EntityVec::from_constrained_values(
                    function
                        .body()
                        .commands()
                        .map(|(_, command)| command.origin())
                        .collect(),
                );
                trace_values.push(FunctionTrace::new(function.resource().clone(), origins));
            }
            Err(error) => findings.push(Diagnostic::new(
                match &error.error {
                    RenderError::CommandTooLong { .. } => "datapack.command-too-long",
                    RenderError::InvalidInternalFunction(_)
                    | RenderError::InvalidInternalTag(_)
                    | RenderError::FormattingFailed => "datapack.render-failed",
                },
                format!(
                    "failed to render {} command {:?}: {}",
                    function.resource(),
                    error.command,
                    error.error
                ),
                error.origin,
            )),
        }
    }

    for (_, tag) in program.function_tags() {
        match serialize_tag(program, tag) {
            Ok(bytes) => files.push(PackFile::new(
                tag.resource().pack_path(program.target()),
                bytes,
            )),
            Err(diagnostic) => findings.push(diagnostic),
        }
    }

    files.sort_by(|left, right| left.path().cmp(right.path()));
    let mut seen = HashSet::with_capacity(files.len());
    for file in &files {
        if !seen.insert(file.path().clone()) {
            findings.push(Diagnostic::new(
                "datapack.path-collision",
                format!("duplicate emitted path {}", file.path()),
                OriginId::UNKNOWN,
            ));
        }
    }

    if let Some(diagnostics) = Diagnostics::from_findings(findings) {
        return Err(diagnostics);
    }

    let trace = TraceMap::new(
        program.target(),
        EntityVec::from_constrained_values(trace_values),
    );
    Ok(EmissionOutput::new(DatapackArtifact::new(files), trace))
}

#[derive(Serialize)]
struct Metadata<'a> {
    pack: MetadataPack<'a>,
}

#[derive(Serialize)]
struct MetadataPack<'a> {
    description: &'a str,
    min_format: [u32; 2],
    max_format: [u32; 2],
}

fn serialize_metadata(
    program: &MinecraftProgram,
    options: &EmissionOptions,
) -> Result<Vec<u8>, serde_json::Error> {
    let format = program.target().spec().data_pack_format();
    let mut bytes = serde_json::to_vec(&Metadata {
        pack: MetadataPack {
            description: options.description(),
            min_format: format,
            max_format: format,
        },
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Serialize)]
struct TagOutput {
    #[serde(skip_serializing_if = "is_false")]
    replace: bool,
    values: Vec<TagValue>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize)]
#[serde(untagged)]
enum TagValue {
    Required(String),
    Optional { id: String, required: bool },
}

fn serialize_tag(
    program: &MinecraftProgram,
    tag: &crate::ir::minecraft::FunctionTag,
) -> Result<Vec<u8>, Diagnostic> {
    let mut values = Vec::with_capacity(tag.entries().len());
    for entry in tag.entries() {
        let (id, requirement) = match entry.kind() {
            FunctionTagEntryKind::Internal(target) => (
                resolve_internal(program, *target).ok_or_else(|| {
                    Diagnostic::new(
                        "datapack.invalid-internal-tag-entry",
                        format!("cannot resolve internal tag entry {target:?}"),
                        entry.origin(),
                    )
                })?,
                ExternalTagRequirement::Required,
            ),
            FunctionTagEntryKind::External {
                target,
                requirement,
            } => (external_text(target), *requirement),
        };
        values.push(match requirement {
            ExternalTagRequirement::Required => TagValue::Required(id),
            ExternalTagRequirement::Optional => TagValue::Optional {
                id,
                required: false,
            },
        });
    }

    let mut bytes = serde_json::to_vec(&TagOutput {
        replace: tag.merge() == FunctionTagMerge::Replace,
        values,
    })
    .map_err(|error| {
        Diagnostic::new(
            "datapack.tag-json",
            format!(
                "failed to serialize function tag {}: {error}",
                tag.resource()
            ),
            tag.origin(),
        )
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn resolve_internal(program: &MinecraftProgram, target: InternalCallableRef) -> Option<String> {
    match target {
        InternalCallableRef::Function(function) => program
            .function(function)
            .map(|value| value.resource().to_string()),
        InternalCallableRef::Tag(tag) => program
            .function_tag(tag)
            .map(|value| format!("#{}", value.resource())),
    }
}

fn external_text(target: &ExternalCallableRef) -> String {
    match target {
        ExternalCallableRef::Function(resource) => resource.to_string(),
        ExternalCallableRef::Tag(resource) => format!("#{resource}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{EmissionOptions, emit_datapack};
    use crate::ir::minecraft::{
        CommandKind, CommandNode, DataCommand, DataModifyMode, DataSource, ExecuteCommand,
        ExecuteModifier, ExecuteModifierKind, ExecuteModifiers, ExternalCallableRef,
        ExternalTagRequirement, FakeScoreHolder, FunctionCall, FunctionResourceId,
        FunctionTagEntry, FunctionTagMerge, FunctionTagResourceId, InternalCallableRef,
        MinecraftProgramBuilder, NbtKey, NbtPath, NbtPathKey, NbtPathSegment, NbtValue,
        ObjectiveName, ScoreCommand, ScoreRef, SingleScoreHolder, StorageId, StoragePath,
        StoreChannel, StoreDestination, UnsafeRawCommand,
    };
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    fn raw(line: &str, origin: OriginId) -> CommandNode {
        CommandNode::new(
            CommandKind::Raw(UnsafeRawCommand::new(line).unwrap()),
            origin,
        )
        .unwrap()
    }

    fn storage(key: &str) -> StoragePath {
        StoragePath::new(
            StorageId::parse("mdl:state").unwrap(),
            NbtPath::new(NbtPathSegment::Key(NbtPathKey::new(key).unwrap()), vec![]),
        )
    }

    fn score(holder: &str) -> ScoreRef {
        ScoreRef::new(
            SingleScoreHolder::from(FakeScoreHolder::new(holder).unwrap()),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
    }

    #[test]
    fn exact_metadata_functions_tags_and_trace_are_deterministic() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:entry").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let empty = builder
            .declare_function(
                FunctionResourceId::parse("mdl:empty").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:entries").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Raw(UnsafeRawCommand::new("say marker").unwrap()),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        builder.begin_function(empty).unwrap().finish();
        let mut entries = builder.begin_function_tag(tag).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(function),
            OriginId::UNKNOWN,
        ));
        entries.push(FunctionTagEntry::external(
            ExternalCallableRef::Function(FunctionResourceId::parse("other:maybe").unwrap()),
            ExternalTagRequirement::Optional,
            OriginId::UNKNOWN,
        ));
        entries.finish();
        let program = builder.finish().unwrap();
        let sources = SourceContext::new();
        let options = EmissionOptions::new("MDL \"test\"");

        let first = emit_datapack(&program, &sources, &options).unwrap();
        let second = emit_datapack(&program, &sources, &options).unwrap();
        assert_eq!(first.pack(), second.pack());
        let paths = first
            .pack()
            .files()
            .iter()
            .map(|file| file.path().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "data/mdl/function/empty.mcfunction",
                "data/mdl/function/entry.mcfunction",
                "data/mdl/tags/function/entries.json",
                "pack.mcmeta"
            ]
        );
        assert_eq!(
            first.pack().file("pack.mcmeta").unwrap().bytes(),
            br#"{"pack":{"description":"MDL \"test\"","min_format":[107,1],"max_format":[107,1]}}
"#
        );
        assert_eq!(
            first
                .pack()
                .file("data/mdl/function/entry.mcfunction")
                .unwrap()
                .bytes(),
            b"say marker\n"
        );
        assert_eq!(
            first
                .pack()
                .file("data/mdl/function/empty.mcfunction")
                .unwrap()
                .bytes(),
            b""
        );
        assert_eq!(
            first
                .pack()
                .file("data/mdl/tags/function/entries.json")
                .unwrap()
                .bytes(),
            br#"{"values":["mdl:entry",{"id":"other:maybe","required":false}]}
"#
        );
        assert_eq!(first.trace().records().count(), 1);
        let record = first.trace().records().next().unwrap();
        assert_eq!(record.function_id(), function);
        assert_eq!(record.line().get(), 1);
        assert_eq!(record.origin(), OriginId::UNKNOWN);
        assert!(first.trace().function(empty).unwrap().is_empty());
    }

    #[test]
    fn checked_emission_rejects_invalid_program_before_rendering() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:bad").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Function(FunctionCall::new(
                    ExternalCallableRef::Function(FunctionResourceId::parse("mdl:bad").unwrap())
                        .into(),
                )),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let diagnostics = emit_datapack(
            &program,
            &SourceContext::new(),
            &EmissionOptions::new("bad"),
        )
        .unwrap_err();
        assert!(diagnostics.contains_code("minecraft.external-aliases-owned-resource"));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn complex_artifacts_preserve_semantic_order_and_top_level_trace() {
        let mut sources = SourceContext::new();
        let top = sources.add_origin(Origin::Unknown).unwrap();
        let nested = sources.add_origin(Origin::Unknown).unwrap();
        let modifier = sources.add_origin(Origin::Unknown).unwrap();

        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(FunctionResourceId::parse("mdl:complex").unwrap(), top)
            .unwrap();
        let leaf = builder
            .declare_function(FunctionResourceId::parse("mdl:leaf").unwrap(), nested)
            .unwrap();
        let empty_tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:empty").unwrap(),
                top,
                FunctionTagMerge::Replace,
            )
            .unwrap();
        let inner_tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:inner").unwrap(),
                top,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let overlap_tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:overlap").unwrap(),
                top,
                FunctionTagMerge::Append,
            )
            .unwrap();

        let value = NbtValue::compound(vec![
            (
                NbtKey::new("z"),
                NbtValue::list(vec![NbtValue::int(2), NbtValue::string("first")]).unwrap(),
            ),
            (NbtKey::new("a"), NbtValue::int(1)),
        ])
        .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Data(DataCommand::Modify {
                    target: storage("ordered"),
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(value),
                }),
                top,
            )
            .unwrap(),
        )
        .unwrap();
        for channel in [StoreChannel::Result, StoreChannel::Success] {
            let run = CommandNode::new(
                CommandKind::Score(ScoreCommand::PlayersGet {
                    score: score("#input"),
                }),
                nested,
            )
            .unwrap();
            let execute = ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::Store(
                            channel,
                            StoreDestination::Score(score(match channel {
                                StoreChannel::Result => "#result",
                                StoreChannel::Success => "#success",
                            })),
                        ),
                        modifier,
                    ),
                    vec![],
                ),
                run,
            );
            body.push(CommandNode::new(CommandKind::Execute(execute), top).unwrap())
                .unwrap();
        }
        body.finish();
        builder.begin_function(leaf).unwrap().finish();
        builder.begin_function_tag(empty_tag).unwrap().finish();

        let mut inner = builder.begin_function_tag(inner_tag).unwrap();
        inner.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(leaf),
            nested,
        ));
        inner.push(FunctionTagEntry::external(
            ExternalCallableRef::Tag(FunctionTagResourceId::parse("other:optional").unwrap()),
            ExternalTagRequirement::Optional,
            nested,
        ));
        inner.finish();

        let mut overlap = builder.begin_function_tag(overlap_tag).unwrap();
        overlap.push(FunctionTagEntry::internal(
            InternalCallableRef::Tag(inner_tag),
            top,
        ));
        overlap.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(leaf),
            top,
        ));
        overlap.finish();

        let program = builder.finish().unwrap();
        let first = emit_datapack(&program, &sources, &EmissionOptions::new("complex")).unwrap();
        let second = emit_datapack(&program, &sources, &EmissionOptions::new("complex")).unwrap();
        assert_eq!(first.pack(), second.pack());
        assert_eq!(
            first
                .pack()
                .file("data/mdl/function/complex.mcfunction")
                .unwrap()
                .bytes(),
            concat!(
                "data modify storage mdl:state \"ordered\" set value {\"a\":1,\"z\":[2,\"first\"]}\n",
                "execute store result score #result mdl.reg run scoreboard players get #input mdl.reg\n",
                "execute store success score #success mdl.reg run scoreboard players get #input mdl.reg\n",
            )
            .as_bytes()
        );
        assert_eq!(
            first
                .pack()
                .file("data/mdl/tags/function/empty.json")
                .unwrap()
                .bytes(),
            b"{\"replace\":true,\"values\":[]}\n"
        );
        assert_eq!(
            first
                .pack()
                .file("data/mdl/tags/function/inner.json")
                .unwrap()
                .bytes(),
            b"{\"values\":[\"mdl:leaf\",{\"id\":\"#other:optional\",\"required\":false}]}\n"
        );
        assert_eq!(
            first
                .pack()
                .file("data/mdl/tags/function/overlap.json")
                .unwrap()
                .bytes(),
            b"{\"values\":[\"#mdl:inner\",\"mdl:leaf\"]}\n"
        );

        let trace = first.trace().function(entry).unwrap();
        assert_eq!(trace.len(), 3);
        assert_eq!(
            first
                .trace()
                .records()
                .filter(|record| record.function_id() == entry)
                .map(|record| (record.line().get(), record.origin()))
                .collect::<Vec<_>>(),
            vec![(1, top), (2, top), (3, top)]
        );
        assert_ne!(top, nested);
        assert_ne!(top, modifier);
    }

    #[test]
    fn rendering_failure_returns_no_partial_artifact() {
        let oversized = "x".repeat(2_000_000);
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:too_long").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Data(DataCommand::Modify {
                    target: storage("value"),
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(NbtValue::string_owned(oversized)),
                }),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let diagnostics = emit_datapack(
            &builder.finish().unwrap(),
            &SourceContext::new(),
            &EmissionOptions::new("failure"),
        )
        .unwrap_err();
        assert!(diagnostics.contains_code("datapack.command-too-long"));
    }

    #[test]
    fn emits_one_hundred_thousand_commands_with_exact_counts() {
        const COMMANDS: usize = 100_000;
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:scale").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        for _ in 0..COMMANDS {
            body.push(raw("say x", OriginId::UNKNOWN)).unwrap();
        }
        body.finish();
        let output = emit_datapack(
            &builder.finish().unwrap(),
            &SourceContext::new(),
            &EmissionOptions::new("scale"),
        )
        .unwrap();
        assert_eq!(output.pack().files().len(), 2);
        assert_eq!(output.trace().function(function).unwrap().len(), COMMANDS);
        assert_eq!(output.trace().records().count(), COMMANDS);
        assert_eq!(
            output
                .pack()
                .file("data/mdl/function/scale.mcfunction")
                .unwrap()
                .bytes()
                .len(),
            COMMANDS * b"say x\n".len()
        );
    }

    #[test]
    #[ignore = "non-gating geometric emission benchmark"]
    fn reports_geometric_emission_scaling_without_a_timing_threshold() {
        for commands in [1_000, 2_000, 4_000, 8_000] {
            let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
            let function = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:benchmark").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let mut body = builder.begin_function(function).unwrap();
            for _ in 0..commands {
                body.push(raw("say x", OriginId::UNKNOWN)).unwrap();
            }
            body.finish();
            let program = builder.finish().unwrap();
            let start = std::time::Instant::now();
            let output = emit_datapack(
                &program,
                &SourceContext::new(),
                &EmissionOptions::new("benchmark"),
            )
            .unwrap();
            eprintln!(
                "commands={commands} bytes={} elapsed={:?}",
                output
                    .pack()
                    .file("data/mdl/function/benchmark.mcfunction")
                    .unwrap()
                    .bytes()
                    .len(),
                start.elapsed()
            );
        }
    }
}
