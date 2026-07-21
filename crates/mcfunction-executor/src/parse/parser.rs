use super::{
    AdvancementCmd, DataCmd, DataSub, ExecuteCmd, ExecuteCondition, ExecuteModifier, ForceloadCmd, FunctionCmd,
    GameruleCmd, ItemCmd, KillCmd, LootCmd, ParsedCommand, ReturnCmd, RotateCmd, ScheduleCmd, ScoreboardCmd,
    ScoreboardSub, SetblockCmd, SummonCmd, TagAction, TagCmd, TeleportCmd, TellrawCmd, TitleCmd,
};

pub fn parse_command(command: &str) -> ParsedCommand {
    let command = command.trim();
    if command.is_empty() {
        return ParsedCommand::Raw(String::new());
    }
    let command = command.strip_prefix('/').unwrap_or(command);
    let (root, rest) = split_first_word(command);
    match root {
        "scoreboard" => parse_scoreboard(rest),
        "data" => parse_data(rest),
        "execute" => parse_execute(rest),
        "function" => parse_function(rest),
        "say" => ParsedCommand::Say(rest.to_owned()),
        "teleport" => parse_teleport(rest),
        "return" => parse_return(rest),
        "gamerule" => parse_gamerule(rest),
        "schedule" => parse_schedule(rest),
        "summon" => parse_summon(rest),
        "forceload" => parse_forceload(rest),
        "setblock" => parse_setblock(rest),
        "kill" => parse_kill(rest),
        "tag" => parse_tag(rest),
        "tellraw" => parse_tellraw(rest),
        "title" => parse_title(rest),
        "playsound" => ParsedCommand::Playsound,
        "loot" => parse_loot(rest),
        "rotate" => parse_rotate(rest),
        "item" => parse_item(rest),
        "advancement" => parse_advancement(rest),
        "reload" => ParsedCommand::Reload,
        "stop" => ParsedCommand::Stop,
        _ => ParsedCommand::Raw(command.to_owned()),
    }
}

fn split_first_word(input: &str) -> (&str, &str) {
    let input = input.trim();
    match input.find(char::is_whitespace) {
        Some(pos) => (&input[..pos], input[pos..].trim()),
        None => (input, ""),
    }
}

fn split_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut in_braces: u32 = 0;
    let mut in_brackets: u32 = 0;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            '{' => {
                in_braces += 1;
                current.push(ch);
            }
            '}' => {
                in_braces = in_braces.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                in_brackets += 1;
                current.push(ch);
            }
            ']' => {
                in_brackets = in_brackets.saturating_sub(1);
                current.push(ch);
            }
            ' ' if !in_quotes && in_braces == 0 && in_brackets == 0 => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn parse_scoreboard(rest: &str) -> ParsedCommand {
    let words = split_words(rest);
    if words.len() < 2 {
        return ParsedCommand::Raw(format!("scoreboard {rest}"));
    }
    match words[0].as_str() {
        "objectives" if words.len() >= 4 && words[1] == "add" => ParsedCommand::Scoreboard(ScoreboardCmd {
            subcommand: ScoreboardSub::ObjectivesAdd {
                objective: words[2].clone(),
                criterion: words[3].clone(),
            },
        }),
        "players" => parse_scoreboard_players(&words),
        _ => ParsedCommand::Raw(format!("scoreboard {rest}")),
    }
}

fn parse_scoreboard_players(words: &[String]) -> ParsedCommand {
    if words.len() < 4 {
        return ParsedCommand::Raw(format!("scoreboard players {}", words.join(" ")));
    }
    let sub = words[1].as_str();
    let holder = words[2].clone();
    let objective = words[3].clone();

    match sub {
        "get" if words.len() >= 4 => ParsedCommand::Scoreboard(ScoreboardCmd {
            subcommand: ScoreboardSub::PlayersGet { holder, objective },
        }),
        "reset" if words.len() >= 4 => ParsedCommand::Scoreboard(ScoreboardCmd {
            subcommand: ScoreboardSub::PlayersReset { holder, objective },
        }),
        "set" if words.len() >= 5 => {
            words.get(4).and_then(|s| s.parse().ok()).map_or_else(
                || ParsedCommand::Raw(format!("scoreboard players {}", words.join(" "))),
                |value| ParsedCommand::Scoreboard(ScoreboardCmd {
                    subcommand: ScoreboardSub::PlayersSet { holder, objective, value },
                }),
            )
        }
        "add" if words.len() >= 5 => {
            words.get(4).and_then(|s| s.parse().ok()).map_or_else(
                || ParsedCommand::Raw(format!("scoreboard players {}", words.join(" "))),
                |amount| ParsedCommand::Scoreboard(ScoreboardCmd {
                    subcommand: ScoreboardSub::PlayersAdd { holder, objective, amount },
                }),
            )
        }
        "remove" if words.len() >= 5 => {
            words.get(4).and_then(|s| s.parse().ok()).map_or_else(
                || ParsedCommand::Raw(format!("scoreboard players {}", words.join(" "))),
                |amount| ParsedCommand::Scoreboard(ScoreboardCmd {
                    subcommand: ScoreboardSub::PlayersRemove { holder, objective, amount },
                }),
            )
        }
        "operation" if words.len() >= 7 => ParsedCommand::Scoreboard(ScoreboardCmd {
            subcommand: ScoreboardSub::PlayersOperation {
                target_holder: holder,
                target_objective: objective,
                op: words[4].clone(),
                source_holder: words[5].clone(),
                source_objective: words[6].clone(),
            },
        }),
        _ => ParsedCommand::Raw(format!("scoreboard players {}", words.join(" "))),
    }
}

fn parse_data(rest: &str) -> ParsedCommand {
    let words = split_words(rest);
    if words.len() < 3 {
        return ParsedCommand::Raw(format!("data {rest}"));
    }
    match words[0].as_str() {
        "get" => parse_data_get(&words),
        "remove" => parse_data_remove(&words),
        "modify" => parse_data_modify(&words),
        "merge" => parse_data_merge(&words),
        _ => ParsedCommand::Raw(format!("data {rest}")),
    }
}

fn parse_data_get(words: &[String]) -> ParsedCommand {
    if words.len() < 2 {
        return ParsedCommand::Raw(format!("data get {}", words.join(" ")));
    }
    // data get storage <id> <path> [scale]
    // data get entity <selector> <path> [scale]
    // data get block <x> <y> <z> <path> [scale]
    if words[1] == "storage" && words.len() >= 3 {
        ParsedCommand::Data(DataCmd {
            subcommand: DataSub::Get {
                storage: words[2].clone(),
                path: words.get(3).cloned().unwrap_or_default(),
                scale: words.get(4).and_then(|s| s.parse().ok()),
            },
        })
    } else if words[1] == "entity" && words.len() >= 3 {
        // data get entity @s Pos[0] → treat as entity:selector path
        ParsedCommand::Data(DataCmd {
            subcommand: DataSub::Get {
                storage: format!("entity:{}", words[2]),
                path: words.get(3).cloned().unwrap_or_default(),
                scale: words.get(4).and_then(|s| s.parse().ok()),
            },
        })
    } else {
        ParsedCommand::Data(DataCmd {
            subcommand: DataSub::Get {
                storage: words[1].clone(),
                path: words.get(2).cloned().unwrap_or_default(),
                scale: words.get(3).and_then(|s| s.parse().ok()),
            },
        })
    }
}

fn parse_data_remove(words: &[String]) -> ParsedCommand {
    if words.len() < 3 || words[1] != "storage" {
        return ParsedCommand::Raw(format!("data remove {}", words.join(" ")));
    }
    ParsedCommand::Data(DataCmd {
        subcommand: DataSub::Remove {
            storage: words[2].clone(),
            path: words.get(3).cloned().unwrap_or_default(),
        },
    })
}

fn parse_data_modify(words: &[String]) -> ParsedCommand {
    if words.len() < 4 || words[1] != "storage" {
        return ParsedCommand::Raw(format!("data modify {}", words.join(" ")));
    }
    let rest = words[4..].join(" ");
    let (mode, source) = split_first_word(&rest);
    ParsedCommand::Data(DataCmd {
        subcommand: DataSub::Modify {
            storage: words[2].clone(),
            path: words[3].clone(),
            mode: mode.to_owned(),
            source: source.to_owned(),
        },
    })
}

fn parse_data_merge(words: &[String]) -> ParsedCommand {
    if words.len() < 4 { return ParsedCommand::Raw(format!("data merge {}", words.join(" "))); }
    let target_kind = &words[1];
    let storage_prefix = match target_kind.as_str() { "entity" => "entity:", _ => "" };
    let storage = format!("{storage_prefix}{}", words[2]);
    let nbt = words[3..].join(" ");
    ParsedCommand::Data(DataCmd {
        subcommand: DataSub::Modify { storage, path: String::new(), mode: "merge".to_owned(), source: format!("value {nbt}") },
    })
}

fn parse_execute(rest: &str) -> ParsedCommand {
    let mut modifiers = Vec::new();
    let mut remaining = rest.to_owned();

    loop {
        let (keyword, next) = split_first_word(&remaining);
        match keyword {
            "run" => {
                return ParsedCommand::Execute(ExecuteCmd {
                    modifiers,
                    nested: Box::new(parse_command(next)),
                });
            }
            "as" => {
                let (sel, r) = split_first_word(next);
                if sel.is_empty() { break; }
                modifiers.push(ExecuteModifier::As(sel.to_owned()));
                remaining = r.to_owned();
            }
            "at" => {
                let (sel, r) = split_first_word(next);
                if sel.is_empty() { break; }
                modifiers.push(ExecuteModifier::At(sel.to_owned()));
                remaining = r.to_owned();
            }
            "in" => {
                let (dim, r) = split_first_word(next);
                modifiers.push(ExecuteModifier::In(dim.to_owned()));
                remaining = r.to_owned();
            }
            "positioned" => {
                let words: Vec<&str> = next.split_whitespace().collect();
                let (count, rest_start) = if words.len() >= 3 && is_coordinate_word(words[0]) {
                    (3, 3)
                } else if words.len() >= 1 && is_coordinate_word(words[0]) {
                    (1, 1)
                } else {
                    (0, 0)
                };
                if count > 0 {
                    let pos = words[..count].join(" ");
                    let r = if rest_start < words.len() { words[rest_start..].join(" ") } else { String::new() };
                    modifiers.push(ExecuteModifier::Positioned(pos));
                    remaining = r;
                } else {
                    break;
                }
            }
            "rotated" => {
                let words: Vec<&str> = next.split_whitespace().collect();
                let count: usize = if words.len() >= 2 && is_coordinate_word(words[0]) { 2 } else { 0 };
                if count > 0 {
                    let rot = words[..count].join(" ");
                    let r = if count < words.len() { words[count..].join(" ") } else { String::new() };
                    modifiers.push(ExecuteModifier::Rotated(rot));
                    remaining = r;
                } else {
                    break;
                }
            }
            "anchored" => {
                let (anchor, r) = split_first_word(next);
                modifiers.push(ExecuteModifier::Anchored(anchor.to_owned()));
                remaining = r.to_owned();
            }
            "align" => {
                let (axes, r) = split_first_word(next);
                modifiers.push(ExecuteModifier::Align(axes.to_owned()));
                remaining = r.to_owned();
            }
            "if" => {
                if let Some((cond, r)) = parse_execute_condition(next) {
                    modifiers.push(ExecuteModifier::If(cond));
                    remaining = r;
                } else { break; }
            }
            "unless" => {
                if let Some((cond, r)) = parse_execute_condition(next) {
                    modifiers.push(ExecuteModifier::Unless(cond));
                    remaining = r;
                } else { break; }
            }
            "store" => {
                let (channel, after_channel) = split_first_word(next);
                let words_in_rest: Vec<&str> = after_channel.split_whitespace().collect();
                let run_pos = words_in_rest.iter().position(|&w| w == "run");
                let dest = match run_pos {
                    Some(pos) => words_in_rest[..pos].join(" "),
                    None => after_channel.to_owned(),
                };
                let run_rest = match run_pos {
                    Some(pos) if pos + 1 < words_in_rest.len() => {
                        format!("run {}", words_in_rest[pos + 1..].join(" "))
                    }
                    Some(_) => "run".to_owned(),
                    None => String::new(),
                };
                match channel {
                    "success" => modifiers.push(ExecuteModifier::StoreSuccess(dest)),
                    "result" => modifiers.push(ExecuteModifier::StoreResult(dest)),
                    _ => break,
                }
                remaining = run_rest;
            }
            "" => break,
            _ => break,
        }
    }

    ParsedCommand::Raw(format!("execute {remaining}"))
}

fn parse_execute_condition(rest: &str) -> Option<(ExecuteCondition, String)> {
    let (kind, rest) = split_first_word(rest);
    match kind {
        "score" => {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if words.len() >= 4 && words[2] == "matches" {
                let range_end = find_execute_keyword_pos(&words[3..]);
                let range_words: Vec<&str> = if range_end == 0 {
                    words[3..].to_vec()
                } else {
                    words[3..3 + range_end].to_vec()
                };
                let remaining = if range_end == 0 || 3 + range_end >= words.len() {
                    String::new()
                } else {
                    words[3 + range_end..].join(" ")
                };
                Some((ExecuteCondition::Score {
                    holder: words[0].to_owned(),
                    objective: words[1].to_owned(),
                    range: range_words.join(" "),
                }, remaining))
            } else if words.len() >= 5 && matches!(words[2], "=" | "<" | "<=" | ">" | ">=") {
                let remaining = if words.len() > 5 { words[5..].join(" ") } else { String::new() };
                Some((ExecuteCondition::ScoreCompare {
                    left_holder: words[0].to_owned(),
                    left_objective: words[1].to_owned(),
                    op: words[2].to_owned(),
                    right_holder: words[3].to_owned(),
                    right_objective: words[4].to_owned(),
                }, remaining))
            } else {
                None
            }
        }
        "data" => {
            let (storage_marker, after_storage) = split_first_word(rest);
            if storage_marker != "storage" {
                return None;
            }
            let (storage, after_id) = split_first_word(after_storage);
            // The next token is either a simple path or a brace-delimited compound {…}
            let after_id = after_id.trim();
            let (path_or_compound, remaining) = if after_id.starts_with('{') {
                // Brace-delimited compound predicate — find matching closing brace
                let end = find_matching_brace(after_id);
                if end == 0 {
                    (after_id.to_owned(), String::new())
                } else {
                    let compound = after_id[..end].to_owned();
                    let rest = after_id[end..].trim().to_owned();
                    (compound, rest)
                }
            } else {
                let (path, r) = split_first_word(after_id);
                (path.to_owned(), r.to_owned())
            };
            Some((ExecuteCondition::Data(storage.to_owned(), path_or_compound), remaining))
        }
        "entity" => {
            let (selector, r) = split_first_word(rest);
            Some((ExecuteCondition::Entity(selector.to_owned()), r.to_owned()))
        }
        "function" => {
            let (name, r) = split_first_word(rest);
            Some((ExecuteCondition::Function(name.to_owned()), r.to_owned()))
        }
        "block" => {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if words.len() >= 4 {
                let x: i32 = words[0].parse().unwrap_or(0);
                let y: i32 = words[1].parse().unwrap_or(0);
                let z: i32 = words[2].parse().unwrap_or(0);
                let block = words[3].to_owned();
                let remaining = if words.len() > 4 { words[4..].join(" ") } else { String::new() };
                Some((ExecuteCondition::Block { x, y, z, block }, remaining))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Find the position of the next execute keyword (`run`, `if`, `unless`, `store`)
/// in a slice of words.
fn find_execute_keyword_pos(words: &[&str]) -> usize {
    for (i, word) in words.iter().enumerate() {
        if *word == "run" || *word == "if" || *word == "unless" || *word == "store" || *word == "as" || *word == "at" {
            return i;
        }
    }
    0
}

fn parse_kill(rest: &str) -> ParsedCommand {
    let (sel, _) = split_first_word(rest);
    if sel.is_empty() { return ParsedCommand::Raw(format!("kill {rest}")); }
    ParsedCommand::Kill(KillCmd { selector: sel.to_owned() })
}
fn parse_tag(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 3 { return ParsedCommand::Raw(format!("tag {rest}")); }
    let a = match w[1].as_str() { "add" => TagAction::Add, "remove" => TagAction::Remove, "list" => TagAction::List, _ => return ParsedCommand::Raw(format!("tag {rest}")) };
    ParsedCommand::Tag(TagCmd { selector: w[0].clone(), action: a, tag: w[2].clone() })
}
fn parse_tellraw(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 2 { return ParsedCommand::Raw(format!("tellraw {rest}")); }
    ParsedCommand::Tellraw(TellrawCmd { selector: w[0].clone(), message: w[1..].join(" ") })
}
fn parse_title(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 3 { return ParsedCommand::Raw(format!("title {rest}")); }
    ParsedCommand::Title(TitleCmd { selector: w[0].clone(), action: w[1].clone(), text: w[2..].join(" ") })
}
fn parse_loot(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 3 { return ParsedCommand::Raw(format!("loot {rest}")); }
    let pos = if w.len() >= 6 { Some((w[1].parse().unwrap_or(0.0), w[2].parse().unwrap_or(0.0), w[3].parse().unwrap_or(0.0))) } else { None };
    ParsedCommand::Loot(LootCmd { action: w[0].to_owned(), pos, source: w.join(" ") })
}
fn parse_rotate(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 2 { return ParsedCommand::Raw(format!("rotate {rest}")); }
    let y = w[1].strip_prefix('~').and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let p = w.get(2).and_then(|s| s.strip_prefix('~')).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    ParsedCommand::Rotate(RotateCmd { selector: w[0].clone(), yaw: y, pitch: p })
}
fn parse_item(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 4 { return ParsedCommand::Raw(format!("item {rest}")); }
    ParsedCommand::Item(ItemCmd { action: w[0].clone(), selector: w[2].clone(), slot: w[3].clone(), rest: w[4..].join(" ") })
}
fn parse_advancement(rest: &str) -> ParsedCommand {
    let w = split_words(rest);
    if w.len() < 4 { return ParsedCommand::Raw(format!("advancement {rest}")); }
    ParsedCommand::Advancement(AdvancementCmd { action: w[0].clone(), selector: w[1].clone(), advancement: w[3..].join(" ") })
}

fn parse_remaining(input: &str) -> (String, String) {
    let input = input.trim();
    let mut depth: u32 = 0;

    for (i, ch) in input.char_indices() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => {
                return (input[..i].to_owned(), input[i..].trim().to_owned());
            }
            _ => {}
        }
    }
    (input.to_owned(), String::new())
}

fn parse_function(rest: &str) -> ParsedCommand {
    let rest = rest.trim();
    let is_tag = rest.starts_with('#');
    let name = if is_tag { &rest[1..] } else { rest };
    ParsedCommand::Function(FunctionCmd { name: name.to_owned(), is_tag, with_storage: None, inline_args: None })
}

fn parse_teleport(rest: &str) -> ParsedCommand {
    let words = split_words(rest);
    if words.len() < 2 {
        return ParsedCommand::Raw(format!("teleport {rest}"));
    }
    ParsedCommand::Teleport(TeleportCmd { target: words[0].clone(), destination: words[1..].join(" ") })
}

fn parse_return(rest: &str) -> ParsedCommand {
    let rest = rest.trim();
    if rest.eq_ignore_ascii_case("fail") {
        return ParsedCommand::Return(ReturnCmd::Fail);
    }
    if let Some(nested) = rest.strip_prefix("run ") {
        return ParsedCommand::Return(ReturnCmd::Run(Box::new(parse_command(nested))));
    }
    if let Ok(value) = rest.parse() {
        return ParsedCommand::Return(ReturnCmd::Value(value));
    }
    ParsedCommand::Raw(format!("return {rest}"))
}

fn parse_gamerule(rest: &str) -> ParsedCommand {
    let (rule, value_str) = split_first_word(rest);
    if let Ok(value) = value_str.parse() {
        ParsedCommand::Gamerule(GameruleCmd { rule: rule.to_owned(), value })
    } else {
        ParsedCommand::Raw(format!("gamerule {rest}"))
    }
}

fn parse_schedule(rest: &str) -> ParsedCommand {
    let rest = rest.trim();
    let Some(after_function) = rest.strip_prefix("function ") else {
        return ParsedCommand::Raw(format!("schedule {rest}"));
    };
    let (func_name, remaining) = split_first_word(after_function);
    let words = split_words(remaining);
    let delay = words.first().and_then(|s| s.trim_end_matches('t').parse().ok()).unwrap_or(0);
    let replace = remaining.contains("replace");

    ParsedCommand::Schedule(ScheduleCmd { function: func_name.to_owned(), delay_ticks: delay, replace })
}

fn parse_summon(rest: &str) -> ParsedCommand {
    let words = split_words(rest);
    if words.is_empty() {
        return ParsedCommand::Raw(format!("summon {rest}"));
    }
    let entity_type = words[0].clone();
    let pos = words.get(3).and_then(|_| {
        Some((words.get(1)?.parse().ok()?, words.get(2)?.parse().ok()?, words.get(3)?.parse().ok()?))
    });
    let nbt = if words.len() > 4 { Some(words[4..].join(" ")) } else { None };
    ParsedCommand::Summon(SummonCmd { entity_type, pos, nbt })
}

fn parse_forceload(rest: &str) -> ParsedCommand {
    let words = split_words(rest);
    let add = words.first().map_or(false, |w| w == "add");
    if words.len() >= 3 {
        let x: Option<i32> = words.get(1).and_then(|s| s.parse().ok());
        let z: Option<i32> = words.get(2).and_then(|s| s.parse().ok());
        if let (Some(x), Some(z)) = (x, z) {
            return ParsedCommand::Forceload(ForceloadCmd { add, x, z });
        }
    }
    ParsedCommand::Raw(format!("forceload {rest}"))
}

fn parse_setblock(rest: &str) -> ParsedCommand {
    let words = split_words(rest);
    if words.len() >= 4 {
        let x: i32 = words[0].parse().unwrap_or(0);
        let y: i32 = words[1].parse().unwrap_or(0);
        let z: i32 = words[2].parse().unwrap_or(0);
        let block = words[3..].join(" ");
        return ParsedCommand::Setblock(SetblockCmd { x, y, z, block });
    }
    ParsedCommand::Raw(format!("setblock {rest}"))
}

fn is_coordinate_word(w: &str) -> bool {
    if w.is_empty() { return false; }
    if w.starts_with('~') || w.starts_with('^') { return true; }
    w.chars().next().map_or(false, |c| c == '-' || c == '.' || c.is_ascii_digit())
}

/// Find the closing brace position in a string starting with `{`.
/// Returns 0 if no match found.
fn find_matching_brace(s: &str) -> usize {
    let mut depth: u32 = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    0
}
