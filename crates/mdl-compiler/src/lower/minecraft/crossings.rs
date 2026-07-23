//! Generic macro crossing engine — COLLECT → FRAME → BRIDGE → RENDER → CALL.
//!
//! Walks any `CommandKind` variant for `Operand::Runtime` markers, derives
//! a `MacroCommand` from the base command structure, bridges runtime values
//! into the macro argument frame, and emits the `FunctionWithStorage` call.

use crate::diagnostic::Diagnostics;
use crate::ir::core::{FunctionId, ValueId};
use crate::ir::minecraft::{
    CommandKind, DataCommand, DataModifyMode, DataSource, ExecuteCommand, ExecuteModifier,
    ExecuteModifierKind, ExecuteModifiers, FiniteF64, FunctionWithStorage, InternalCallableRef,
    MacroArguments, MacroCommand, MacroLine, MacroSegment, MacroVariable, MacroVariableId,
    McFunctionId, NbtPath, NbtPathKey, NbtPathSegment, NbtValue, ScoreCommand, StorageId,
    StorageNumericType, StoragePath, StoreChannel, StoreDestination, SyntaxSlot,
};

use crate::source::OriginId;

use super::construct::{FunctionLoweringCx, command, invariant_diagnostics};
use super::plan::LoweringPlan;

/// One runtime operand collected from a command's operand tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeOperand {
    pub value_id: ValueId,
    pub slot: SyntaxSlot,
}

/// A prepared macro argument frame with the mapping from `ValueId` → key.
pub(crate) struct MacroFrame {
    pub arguments: MacroArguments,
    pub variables: Vec<(ValueId, String, MacroVariableId)>,
}

// ── COLLECT ──

/// Collects every `Operand::Runtime` slot in a command. Extension points for
/// other `CommandKind` variants are documented match arms.
pub(crate) fn collect_runtime_operands(command: &CommandKind) -> Vec<RuntimeOperand> {
    match command {
        CommandKind::Data(DataCommand::Modify { source, .. }) => collect_from_data_source(source),
        CommandKind::ItemReplaceBlock(command) => collect_from_item_replace_block(command),
        _ => vec![],
    }
}

/// PS-16, BE-2: `container.<slot>` accepts a runtime slot the same way an
/// entity-NBT `Match` segment does; `<item>`/`<count>` are ordinary
/// command-argument positions a runtime `String`/`Int32` value can also
/// fill. Collected in `slot`, `item`, `count` order — an arbitrary but
/// stable order, since `build_frame` deduplicates by `ValueId` regardless.
fn collect_from_item_replace_block(
    command: &crate::ir::minecraft::ItemReplaceBlockCommand,
) -> Vec<RuntimeOperand> {
    let mut operands = Vec::with_capacity(3);
    if let crate::ir::core::Operand::Runtime(value_id) = command.slot() {
        operands.push(RuntimeOperand {
            value_id,
            slot: SyntaxSlot::NbtIndex,
        });
    }
    if let crate::ir::core::Operand::Runtime(value_id) = command.item_id() {
        operands.push(RuntimeOperand {
            value_id: *value_id,
            slot: SyntaxSlot::ResourceId,
        });
    }
    if let crate::ir::core::Operand::Runtime(value_id) = command.count() {
        operands.push(RuntimeOperand {
            value_id,
            slot: SyntaxSlot::Int,
        });
    }
    operands
}

fn collect_from_data_source(source: &DataSource) -> Vec<RuntimeOperand> {
    match source {
        DataSource::Entity { path, .. } => collect_from_nbt_path_segments(path.segments()),
        _ => vec![],
    }
}

fn collect_from_nbt_path_segments(segments: &[NbtPathSegment]) -> Vec<RuntimeOperand> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            NbtPathSegment::Index(crate::ir::core::Operand::Runtime(v)) => Some(RuntimeOperand {
                value_id: *v,
                slot: SyntaxSlot::NbtIndex,
            }),
            _ => None,
        })
        .collect()
}

// ── FRAME ──

/// Builds a macro argument frame from collected runtime operands, deduplicating
/// by `ValueId` and assigning stable keys `i0`, `i1`, … .
pub(crate) fn build_frame(operands: &[RuntimeOperand]) -> Option<MacroFrame> {
    if operands.is_empty() {
        return None;
    }

    let mut variables: Vec<(ValueId, String, MacroVariableId)> = Vec::new();
    let mut seen: std::collections::BTreeMap<ValueId, usize> = std::collections::BTreeMap::new();

    for op in operands {
        if let Some(&existing_idx) = seen.get(&op.value_id) {
            // Reuse existing key for deduplicated ValueId
            let (_, ref existing_key, MacroVariableId(var_id)) = variables[existing_idx];
            let duplicate_id = MacroVariableId(var_id + 1);
            variables.push((op.value_id, existing_key.clone(), duplicate_id));
        } else {
            let key = format!("i{}", seen.len());
            let var_id = MacroVariableId(u16::try_from(variables.len()).unwrap_or(u16::MAX));
            seen.insert(op.value_id, variables.len());
            variables.push((op.value_id, key, var_id));
        }
    }

    let macro_variables: Vec<MacroVariable> = variables
        .iter()
        .map(|(_, key, _)| MacroVariable {
            key: key.clone(),
            slot: SyntaxSlot::NbtIndex,
            source: StoragePath::new(
                StorageId::parse("mdl:__mdl/macro").expect("valid"),
                NbtPath::new(
                    NbtPathSegment::Key(NbtPathKey::new("args").expect("valid")),
                    vec![NbtPathSegment::Key(NbtPathKey::new(key).expect("valid"))],
                ),
            ),
        })
        .collect();

    let arguments = MacroArguments::new(macro_variables)
        .expect("auto-generated macro variable keys are valid and unique");

    Some(MacroFrame {
        arguments,
        variables,
    })
}

// ── RENDER ──

/// Converts a base command into a `MacroCommand` using the frame's variable
/// mapping. Today handles `DataCommand::Modify` with an entity source whose
/// NBT path contains `Operand::Runtime` indices.
pub(crate) fn render_as_macro(
    base_command: &CommandKind,
    frame: &MacroFrame,
    origin: OriginId,
) -> MacroCommand {
    match base_command {
        CommandKind::Data(DataCommand::Modify { target, source, .. }) => {
            render_data_modify_as_macro(target, source, frame, origin)
        }
        CommandKind::ItemReplaceBlock(command) => {
            render_item_replace_block_as_macro(command, frame, origin)
        }
        _ => panic!("render_as_macro called on unsupported CommandKind variant"),
    }
}

/// Builds the macro-routed whole-slot write (PS-16, BE-2): a single
/// `item replace block <pos> container.<slot> with <item> <count>` line,
/// substituting whichever of `slot`/`item_id`/`count` are `Operand::Runtime`
/// — much simpler than the read side's multi-line NBT-path rendering, since
/// a write is always exactly one flat command with no default/fallback line.
fn render_item_replace_block_as_macro(
    command: &crate::ir::minecraft::ItemReplaceBlockCommand,
    frame: &MacroFrame,
    origin: OriginId,
) -> MacroCommand {
    use std::fmt::Write;

    let position = command.position();
    let mut segments: Vec<MacroSegment> = Vec::new();
    let mut literal_buf = String::new();
    write!(
        literal_buf,
        "item replace block {} {} {} container.",
        position.x, position.y, position.z
    )
    .unwrap();

    match command.slot() {
        crate::ir::core::Operand::Const(slot) => write!(literal_buf, "{slot}").unwrap(),
        crate::ir::core::Operand::Runtime(value) => {
            segments.push(MacroSegment::Literal(std::mem::take(&mut literal_buf)));
            segments.push(MacroSegment::Variable(macro_variable_for(frame, value)));
        }
    }

    literal_buf.push_str(" with ");
    match command.item_id() {
        crate::ir::core::Operand::Const(item_id) => literal_buf.push_str(item_id),
        crate::ir::core::Operand::Runtime(value) => {
            segments.push(MacroSegment::Literal(std::mem::take(&mut literal_buf)));
            segments.push(MacroSegment::Variable(macro_variable_for(frame, *value)));
        }
    }

    literal_buf.push(' ');
    match command.count() {
        crate::ir::core::Operand::Const(count) => write!(literal_buf, "{count}").unwrap(),
        crate::ir::core::Operand::Runtime(value) => {
            segments.push(MacroSegment::Literal(std::mem::take(&mut literal_buf)));
            segments.push(MacroSegment::Variable(macro_variable_for(frame, value)));
        }
    }

    if !literal_buf.is_empty() {
        segments.push(MacroSegment::Literal(literal_buf));
    }

    MacroCommand::new(
        vec![MacroLine { segments }],
        frame.arguments.clone(),
        origin,
    )
    .expect("auto-generated macro command is valid")
}

/// Looks up the macro variable id bridging a runtime `ValueId`, built by
/// `build_frame`. Shared by every macro-line renderer.
fn macro_variable_for(frame: &MacroFrame, value: ValueId) -> MacroVariableId {
    frame
        .variables
        .iter()
        .find(|(v, _, _)| *v == value)
        .map(|(_, _, id)| *id)
        .expect("runtime operand not found in macro frame")
}

fn render_data_modify_as_macro(
    target: &StoragePath,
    source: &DataSource,
    frame: &MacroFrame,
    origin: OriginId,
) -> MacroCommand {
    // Line 1: empty init — data modify storage <target> <path> set value ""
    let empty_init = MacroLine {
        segments: vec![MacroSegment::Literal(format!(
            "data modify storage {} {} set value \"\"",
            target.storage(),
            target.path()
        ))],
    };

    // Line 2: the actual read, with path segments substituted
    let read_line = build_entity_nbt_read_line(target.storage(), target.path(), source, frame);

    MacroCommand::new(vec![empty_init, read_line], frame.arguments.clone(), origin)
        .expect("auto-generated macro command is valid")
}

/// Builds the `data modify storage <target> <path> set from entity/block ...
/// <source path, with any runtime Index/Match segment substituted>` line
/// shared by every entity-NBT macro read shape (BE-1's `Chest`, PS-12's
/// `written_book_content`). Split out of `render_data_modify_as_macro` so
/// the scalar (`Bool`/`I32`) scratch-conversion shape
/// (`render_entity_nbt_scalar_read_as_macro`) can reuse it for its own
/// middle line instead of duplicating the segment-rendering loop.
#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive per-segment-kind match keeps every path-rendering case (both \
              DataSource variants, both Index and Match, const and runtime) local and easy to \
              audit together, rather than split across smaller functions that would each need \
              the same frame/literal-buffer threading"
)]
fn build_entity_nbt_read_line(
    target_storage: &StorageId,
    target_path: &NbtPath,
    source: &DataSource,
    frame: &MacroFrame,
) -> MacroLine {
    use std::fmt::Write;

    let mut segments: Vec<MacroSegment> = Vec::new();
    let mut literal_buf = String::new();
    let path = match source {
        DataSource::Entity { selector, path } => {
            write!(
                literal_buf,
                "data modify storage {target_storage} {target_path} set from entity {selector} "
            )
            .unwrap();
            path
        }
        DataSource::Block { position, path } => {
            write!(
                literal_buf,
                "data modify storage {target_storage} {target_path} set from block {} {} {} ",
                position.x, position.y, position.z
            )
            .unwrap();
            path
        }
        _ => panic!("unsupported data source for macro rendering"),
    };

    for (index, segment) in path.segments().iter().enumerate() {
        match segment {
            NbtPathSegment::Key(key) => {
                let key_str = key.as_str();
                let separator = if index == 0 { "" } else { "." };
                if key_str
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    write!(literal_buf, "{separator}{key_str}").unwrap();
                } else {
                    write!(
                        literal_buf,
                        "{separator}\"{}\"",
                        key_str.replace('\\', "\\\\").replace('"', "\\\"")
                    )
                    .unwrap();
                }
            }
            NbtPathSegment::Index(crate::ir::core::Operand::Runtime(value)) => {
                // Flush accumulated literal
                if !literal_buf.is_empty() {
                    segments.push(MacroSegment::Literal(std::mem::take(&mut literal_buf)));
                }
                let var_id = frame
                    .variables
                    .iter()
                    .find(|(v, _, _)| *v == *value)
                    .map(|(_, _, id)| *id)
                    .expect("runtime operand not found in macro frame");
                segments.push(MacroSegment::Literal("[".to_owned()));
                segments.push(MacroSegment::Variable(var_id));
                segments.push(MacroSegment::Literal("]".to_owned()));
            }
            NbtPathSegment::Index(crate::ir::core::Operand::Const(n)) => {
                write!(literal_buf, "[{n}]").unwrap();
            }
            NbtPathSegment::AllElements => {
                literal_buf.push_str("[]");
            }
            NbtPathSegment::Match {
                key,
                value: crate::ir::core::Operand::Const(n),
                value_kind,
            } => {
                write!(
                    literal_buf,
                    "[{{{}:{n}{}}}]",
                    key.as_str(),
                    match_value_suffix(*value_kind)
                )
                .unwrap();
            }
            NbtPathSegment::Match {
                key,
                value: crate::ir::core::Operand::Runtime(value),
                value_kind,
            } => {
                // A macro `$(key)` substitution always renders as bare
                // decimal text regardless of the NBT tag the bridged value
                // was stored with (measured against the real server, not
                // assumed — see `block-entity-nbt-paths.md`'s Slice 2 note):
                // the type suffix must be a literal character in the
                // command *template*, immediately after the substitution
                // marker, not derived from the stored macro-argument value.
                if !literal_buf.is_empty() {
                    segments.push(MacroSegment::Literal(std::mem::take(&mut literal_buf)));
                }
                let var_id = frame
                    .variables
                    .iter()
                    .find(|(v, _, _)| *v == *value)
                    .map(|(_, _, id)| *id)
                    .expect("runtime operand not found in macro frame");
                segments.push(MacroSegment::Literal(format!("[{{{}:", key.as_str())));
                segments.push(MacroSegment::Variable(var_id));
                segments.push(MacroSegment::Literal(format!(
                    "{}}}]",
                    match_value_suffix(*value_kind)
                )));
            }
        }
    }

    if !literal_buf.is_empty() {
        segments.push(MacroSegment::Literal(literal_buf));
    }

    MacroLine { segments }
}

const fn match_value_suffix(value_kind: crate::ir::minecraft::NbtMatchValueKind) -> &'static str {
    match value_kind {
        crate::ir::minecraft::NbtMatchValueKind::Byte => "b",
        crate::ir::minecraft::NbtMatchValueKind::Int32 => "",
    }
}

/// Builds the macro-routed `Bool`/`I32` entity-NBT read shape (BE-1 Slice 2):
/// a type-appropriate default write to the shared scratch slot, the
/// macro-substituted read attempt into that same slot (reusing
/// `build_entity_nbt_read_line`, exactly like the `String` shape does), and
/// a plain (non-macro — it names no runtime value, only the fixed scratch
/// location) score-store conversion, all three lines in one macro command
/// so the whole read stays inside a single helper-function body. Mirrors
/// `emit_entity_nbt_read_result`'s inline scalar shape one level up, across
/// the macro-helper boundary instead of within one function.
pub(crate) fn render_entity_nbt_scalar_read_as_macro(
    scratch: &StoragePath,
    source: &DataSource,
    default: &crate::ir::minecraft::NbtValue,
    score: &crate::ir::minecraft::ScoreRef,
    frame: &MacroFrame,
    origin: OriginId,
) -> MacroCommand {
    let default_init = MacroLine {
        segments: vec![MacroSegment::Literal(format!(
            "data modify storage {} {} set value {default}",
            scratch.storage(),
            scratch.path()
        ))],
    };
    let read_attempt = build_entity_nbt_read_line(scratch.storage(), scratch.path(), source, frame);
    let convert = MacroLine {
        segments: vec![MacroSegment::Literal(format!(
            "execute store result score {score} run data get storage {} {}",
            scratch.storage(),
            scratch.path()
        ))],
    };
    MacroCommand::new(
        vec![default_init, read_attempt, convert],
        frame.arguments.clone(),
        origin,
    )
    .expect("auto-generated macro command is valid")
}

// ── BRIDGE ──

/// Emits a bridge command for every runtime operand in the frame, writing
/// directly into the seeded `mdl:__mdl/macro args` compound. Score-homed
/// values (`Bool`/`I32`, PS-11's original and still most common case) bridge
/// via `execute store result storage … run scoreboard players get …`.
/// Storage-homed values (`String` — PS-16, BE-2's runtime item id is the
/// first user of this) bridge via a direct `data modify storage … set from
/// storage …` copy instead: a string was never score-representable, so
/// there is no score to read from.
pub(crate) fn emit_bridges(
    context: &mut FunctionLoweringCx<'_, '_>,
    plan: &LoweringPlan,
    function: FunctionId,
    frame: &MacroFrame,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let arg_storage = StorageId::parse("mdl:__mdl/macro").expect("valid");

    for (value_id, key, _) in &frame.variables {
        let home = plan.value_home(function, *value_id).ok_or_else(|| {
            invariant_diagnostics("reachable Core value has no planned home", origin)
        })?;
        let target = StoragePath::new(
            arg_storage.clone(),
            NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("args").expect("valid")),
                vec![NbtPathSegment::Key(NbtPathKey::new(key).expect("valid"))],
            ),
        );

        let bridge = if let Some(crate::ir::core::CoreType::String) = plan.home_type(home) {
            let source = plan.string_storage(home).ok_or_else(|| {
                invariant_diagnostics(
                    "runtime String macro operand has no planned string storage",
                    origin,
                )
            })?;
            command(
                CommandKind::Data(DataCommand::Modify {
                    target,
                    mode: DataModifyMode::Set,
                    source: DataSource::From(source),
                }),
                origin,
            )?
        } else {
            let score_ref = context.score(home)?;
            let store_mod = ExecuteModifier::new(
                ExecuteModifierKind::Store(
                    StoreChannel::Result,
                    StoreDestination::Storage {
                        target,
                        numeric_type: StorageNumericType::Int,
                        scale: FiniteF64::new(1.0).expect("one is finite"),
                    },
                ),
                origin,
            );
            let get_cmd = command(
                CommandKind::Score(ScoreCommand::PlayersGet { score: score_ref }),
                origin,
            )?;
            command(
                CommandKind::Execute(ExecuteCommand::new(
                    ExecuteModifiers::new(store_mod, vec![]),
                    get_cmd,
                )),
                origin,
            )?
        };
        context.push(bridge)?;
    }

    Ok(())
}

// ── CALL ──

/// Seeds the empty macro argument compound. Returns the argument `StoragePath`
/// for use by bridge emitters. The caller must push `FunctionWithStorage` after
/// all bridges have been emitted (seed → bridges → call).
pub(crate) fn seed_macro_frame(
    context: &mut FunctionLoweringCx<'_, '_>,
    origin: OriginId,
) -> Result<StoragePath, Diagnostics> {
    let arg_storage = StorageId::parse("mdl:__mdl/macro").expect("valid");
    let arg_path = NbtPath::new(
        NbtPathSegment::Key(NbtPathKey::new("args").expect("valid")),
        vec![],
    );
    let args = StoragePath::new(arg_storage, arg_path);

    context.push(command(
        CommandKind::Data(DataCommand::Modify {
            target: args.clone(),
            mode: DataModifyMode::Set,
            source: crate::ir::minecraft::DataSource::Value(NbtValue::compound(vec![]).unwrap()),
        }),
        origin,
    )?)?;

    Ok(args)
}

/// Pushes the `function <target> with storage <args>` command.
pub(crate) fn push_macro_call(
    context: &mut FunctionLoweringCx<'_, '_>,
    target: McFunctionId,
    args: StoragePath,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.push(command(
        CommandKind::FunctionWithStorage(FunctionWithStorage::new(
            InternalCallableRef::Function(target).into(),
            args,
        )),
        origin,
    )?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RuntimeOperand, build_frame, render_as_macro};
    use crate::entity::EntityId;
    use crate::ir::core::{Operand, ValueId};
    use crate::ir::minecraft::{
        AtMostOneSelector, CommandKind, DataCommand, DataModifyMode, DataSource, MacroSegment,
        NbtPath, NbtPathKey, NbtPathSegment, Selector, StorageId, StoragePath, SyntaxSlot,
    };
    use crate::source::OriginId;

    // PS-12.0 regression: the macro renderer must honor the real
    // `DataSource::Entity` selector, not a hardcoded `@s` literal. Reads
    // through a non-executor selector (e.g. a general entity-NBT path root)
    // would silently render the wrong entity's data otherwise.
    #[test]
    fn render_data_modify_as_macro_honors_non_self_selector() {
        let value = ValueId::from_index(0);
        let target = StoragePath::new(
            StorageId::parse("mdl:test").unwrap(),
            NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("result").unwrap()),
                vec![],
            ),
        );
        let source = DataSource::Entity {
            selector: Selector::from(AtMostOneSelector::NearestPlayer),
            path: NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("pages").unwrap()),
                vec![NbtPathSegment::Index(Operand::Runtime(value))],
            ),
        };
        let base_cmd = CommandKind::Data(DataCommand::Modify {
            target,
            mode: DataModifyMode::Set,
            source,
        });
        let operands = vec![RuntimeOperand {
            value_id: value,
            slot: SyntaxSlot::NbtIndex,
        }];
        let frame = build_frame(&operands).expect("one runtime operand builds a frame");
        let macro_cmd = render_as_macro(&base_cmd, &frame, OriginId::UNKNOWN);

        let rendered = macro_cmd
            .lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .filter_map(|segment| match segment {
                MacroSegment::Literal(text) => Some(text.as_str()),
                MacroSegment::Variable(_) => None,
            })
            .collect::<String>();

        assert!(
            rendered.contains("entity @p "),
            "expected the real selector `@p` in the rendered macro text: {rendered}"
        );
        assert!(
            !rendered.contains("@s"),
            "rendered macro text still hardcodes `@s`: {rendered}"
        );
    }

    // Real-server regression, found manually while verifying PS-12E: a
    // multi-segment path's root key must not carry a leading `.` (invalid
    // NBT path syntax; Minecraft's parser rejects `.equipment...` with
    // "Invalid NBT path element", but accepts `equipment...`). The prior
    // loop always wrote `.{key}` for every `Key` segment including the
    // first, which no earlier test caught since every earlier fixture's
    // multi-segment path was only ever checked by substring pattern
    // (`contains("pages[$(i0)]")`), never by an execution-validity check,
    // and this unit test's own sibling above uses a single-segment path
    // that can't expose an extra leading separator on the root.
    #[test]
    fn render_data_modify_as_macro_does_not_prefix_the_root_key_with_a_dot() {
        let value = ValueId::from_index(0);
        let target = StoragePath::new(
            StorageId::parse("mdl:test").unwrap(),
            NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("result").unwrap()),
                vec![],
            ),
        );
        let source = DataSource::Entity {
            selector: Selector::from(AtMostOneSelector::SelfExecutor),
            path: NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("equipment").unwrap()),
                vec![
                    NbtPathSegment::Key(NbtPathKey::new("mainhand").unwrap()),
                    NbtPathSegment::Key(NbtPathKey::new("pages").unwrap()),
                    NbtPathSegment::Index(Operand::Runtime(value)),
                ],
            ),
        };
        let base_cmd = CommandKind::Data(DataCommand::Modify {
            target,
            mode: DataModifyMode::Set,
            source,
        });
        let operands = vec![RuntimeOperand {
            value_id: value,
            slot: SyntaxSlot::NbtIndex,
        }];
        let frame = build_frame(&operands).expect("one runtime operand builds a frame");
        let macro_cmd = render_as_macro(&base_cmd, &frame, OriginId::UNKNOWN);

        let rendered = macro_cmd
            .lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .filter_map(|segment| match segment {
                MacroSegment::Literal(text) => Some(text.as_str()),
                MacroSegment::Variable(_) => None,
            })
            .collect::<String>();

        assert!(
            rendered.contains("entity @s equipment.mainhand.pages["),
            "root key must not be preceded by a separator dot: {rendered}"
        );
        assert!(
            !rendered.contains(".equipment"),
            "root key still carries an invalid leading dot: {rendered}"
        );
    }
}
