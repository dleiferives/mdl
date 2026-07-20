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
        _ => vec![],
    }
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
        _ => panic!("render_as_macro called on unsupported CommandKind variant"),
    }
}

fn render_data_modify_as_macro(
    target: &StoragePath,
    source: &DataSource,
    frame: &MacroFrame,
    origin: OriginId,
) -> MacroCommand {
    use std::fmt::Write;

    let target_storage = target.storage();
    let target_path = target.path();

    // Line 1: empty init — data modify storage <target> <path> set value ""
    let empty_init = MacroLine {
        segments: vec![MacroSegment::Literal(format!(
            "data modify storage {target_storage} {target_path} set value \"\""
        ))],
    };

    // Line 2: the actual read, with path segments substituted
    let read_line = match source {
        DataSource::Entity { selector, path } => {
            let mut segments: Vec<MacroSegment> = Vec::new();
            let mut literal_buf = String::new();
            write!(
                literal_buf,
                "data modify storage {target_storage} {target_path} set from entity {selector} "
            )
            .unwrap();

            for segment in path.segments() {
                match segment {
                    NbtPathSegment::Key(key) => {
                        let key_str = key.as_str();
                        if key_str
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        {
                            write!(literal_buf, ".{key_str}").unwrap();
                        } else {
                            write!(
                                literal_buf,
                                ".\"{}\"",
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
                }
            }

            if !literal_buf.is_empty() {
                segments.push(MacroSegment::Literal(std::mem::take(&mut literal_buf)));
            }

            MacroLine { segments }
        }
        _ => panic!("unsupported data source for macro rendering"),
    };

    MacroCommand::new(vec![empty_init, read_line], frame.arguments.clone(), origin)
        .expect("auto-generated macro command is valid")
}

// ── BRIDGE ──

/// Emits `execute store result storage … run scoreboard players get …` bridges
/// for every runtime operand in the frame, writing directly into the seeded
/// `mdl:__mdl/macro args` compound.
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
            invariant_diagnostics("reachable Core value has no planned score home", origin)
        })?;
        let score_ref = context.score(home)?;

        let target = StoragePath::new(
            arg_storage.clone(),
            NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("args").expect("valid")),
                vec![NbtPathSegment::Key(NbtPathKey::new(key).expect("valid"))],
            ),
        );

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
            CommandKind::Score(ScoreCommand::PlayersGet {
                score: score_ref.clone(),
            }),
            origin,
        )?;

        context.push(command(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(store_mod, vec![]),
                get_cmd,
            )),
            origin,
        )?)?;
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
}
