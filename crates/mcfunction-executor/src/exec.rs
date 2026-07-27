use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::parse::{
    CommandOutcome, ExecuteCmd, ExecuteCondition, ExecuteModifier, ParsedCommand, ReturnCmd,
    ScheduleCmd, ScoreboardSub, TagAction, parse_command,
};
use crate::state::scoreboard::ScoreOp;
use crate::state::{NbtValue, World};
use crate::version::MinecraftVersion;

/// The main command executor — reads datapacks, parses mcfunction text,
/// runs commands against in-memory state with tick-based scheduling.
#[derive(Debug)]
pub struct Executor {
    pub world: World,
    /// All loaded functions, keyed by resource identifier.
    functions: HashMap<String, FunctionBody>,
    /// Function tags, resolved to ordered function lists.
    function_tags: HashMap<String, Vec<String>>,
    /// Roots waiting to execute this tick.
    roots: VecDeque<ExecutionRoot>,
    /// Pending scheduled function invocations for future ticks.
    scheduled: Vec<PendingSchedule>,
    /// All log output produced so far.
    pub log: Vec<String>,
    /// Commands that parsed but are not modeled by this executor.
    unsupported_commands: Vec<String>,
    root: PathBuf,
    pub current_dimension: String,
}

#[derive(Clone, Debug)]
struct FunctionBody {
    commands: Vec<String>,
}

#[derive(Clone, Debug)]
enum ExecutionRoot {
    Console(String),
    Scheduled(String),
}

#[derive(Clone, Debug)]
struct PendingSchedule {
    ticks_remaining: u32,
    function: String,
}

/// Budget tracking for command sequence and fork limits.
#[derive(Clone, Debug)]
struct CommandBudget {
    sequence_remaining: u32,
    fork_limit: u32,
}

impl Executor {
    #[must_use]
    pub fn new(root: PathBuf, version: MinecraftVersion) -> Self {
        Self {
            world: World::new(version),
            functions: HashMap::new(),
            function_tags: HashMap::new(),
            roots: VecDeque::new(),
            scheduled: Vec::new(),
            log: Vec::new(),
            unsupported_commands: Vec::new(),
            root,
            current_dimension: "minecraft:overworld".to_owned(),
        }
    }

    /// Scans the sandbox `world/datapacks/` directory and loads all function
    /// `.mcfunction` files and tag JSONs into memory.  Executes the
    /// `minecraft:load` function tag so that pack initialization runs.
    pub fn load_datapacks(&mut self) -> Result<(), String> {
        let datapacks_dir = self.root.join("world").join("datapacks");
        if !datapacks_dir.is_dir() {
            return Ok(());
        }
        for pack_dir in sorted_directory_paths(&datapacks_dir, "datapacks")? {
            if !pack_dir.is_dir() {
                continue;
            }
            self.load_datapack(&pack_dir)?;
        }
        self.log
            .push("[Server thread/INFO]: Loaded datapacks".to_owned());

        // Execute minecraft:load tag
        if self.function_tags.contains_key("minecraft:load") {
            self.roots.push_back(ExecutionRoot::Console(
                "function #minecraft:load".to_owned(),
            ));
            self.run_ready_roots();
        }
        Ok(())
    }

    fn load_datapack(&mut self, pack_dir: &Path) -> Result<(), String> {
        let data_dir = pack_dir.join("data");
        if !data_dir.is_dir() {
            return Ok(());
        }
        for ns_dir in sorted_directory_paths(&data_dir, "namespaces")? {
            if !ns_dir.is_dir() {
                continue;
            }
            let namespace = ns_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            self.load_namespace(namespace, &ns_dir)?;
        }
        Ok(())
    }

    fn load_namespace(&mut self, namespace: &str, ns_dir: &Path) -> Result<(), String> {
        let functions_dir = ns_dir.join("function");
        if functions_dir.is_dir() {
            self.load_functions(namespace, &functions_dir, "")?;
        }
        let tags_dir = ns_dir.join("tags").join("function");
        if tags_dir.is_dir() {
            self.load_function_tags(namespace, &tags_dir)?;
        }
        Ok(())
    }

    fn load_functions(&mut self, namespace: &str, dir: &Path, prefix: &str) -> Result<(), String> {
        for path in sorted_directory_paths(dir, "functions")? {
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let resource = if prefix.is_empty() {
                format!("{namespace}:{name}")
            } else {
                format!("{namespace}:{prefix}/{name}")
            };

            if path.is_dir() {
                let new_prefix = if prefix.is_empty() {
                    name.to_owned()
                } else {
                    format!("{prefix}/{name}")
                };
                self.load_functions(namespace, &path, &new_prefix)?;
            } else if path.extension().is_some_and(|ext| ext == "mcfunction") {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("read function {resource}: {e}"))?;
                let commands: Vec<String> = content
                    .lines()
                    .map(|l| l.trim().to_owned())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .collect();
                self.functions
                    .insert(resource.clone(), FunctionBody { commands });
            }
        }
        Ok(())
    }

    fn load_function_tags(&mut self, namespace: &str, dir: &Path) -> Result<(), String> {
        for path in sorted_directory_paths(dir, "function tags")? {
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let resource = format!("{namespace}:{name}");

            if path.extension().is_some_and(|ext| ext == "json") {
                let content =
                    fs::read_to_string(&path).map_err(|e| format!("read tag {resource}: {e}"))?;
                let tag: serde_json::Value = serde_json::from_str(&content)
                    .map_err(|error| format!("parse tag {resource}: {error}"))?;
                let values = tag
                    .get("values")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| format!("tag {resource} is missing a `values` array"))?;
                let entries: Vec<String> = values
                    .iter()
                    .filter_map(|value| match value {
                        serde_json::Value::String(id) => Some(id.clone()),
                        serde_json::Value::Object(entry) => entry
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(String::from),
                        _ => None,
                    })
                    .collect();
                self.function_tags.insert(resource, entries);
            }
        }
        Ok(())
    }

    /// Enqueues a console command and runs all work ready in the current tick.
    pub fn console_command(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.roots
            .push_back(ExecutionRoot::Console(command.to_owned()));
        self.run_ready_roots();
        Ok(std::mem::take(&mut self.log))
    }

    /// Advances the world by the given number of ticks, processing all roots.
    pub fn advance_ticks(&mut self, ticks: u64) -> Result<(), String> {
        for _ in 0..ticks {
            self.world.tick += 1;
            self.advance_scheduled();
            self.run_ready_roots();
        }
        Ok(())
    }

    fn run_ready_roots(&mut self) {
        while !self.roots.is_empty() {
            self.process_roots();
        }
    }

    fn process_roots(&mut self) {
        let roots: Vec<ExecutionRoot> = self.roots.drain(..).collect();
        for root in roots {
            let budget = CommandBudget {
                sequence_remaining: u32::try_from(
                    self.world
                        .gamerules
                        .rules
                        .get("max_command_sequence_length")
                        .copied()
                        .unwrap_or(65_536),
                )
                .unwrap_or(0),
                fork_limit: u32::try_from(
                    self.world
                        .gamerules
                        .rules
                        .get("max_command_forks")
                        .copied()
                        .unwrap_or(65_536),
                )
                .unwrap_or(0),
            };
            let outcome = match root {
                ExecutionRoot::Console(cmd) => self.execute_console_line(&cmd, &budget),
                ExecutionRoot::Scheduled(func) => self.execute_function_root(&func, &budget),
            };
            self.log.extend(outcome.log);
        }
    }

    fn advance_scheduled(&mut self) {
        let mut ready = Vec::new();
        let mut remaining = Vec::new();
        for mut sched in self.scheduled.drain(..) {
            if sched.ticks_remaining == 0 {
                ready.push(sched.function);
            } else {
                sched.ticks_remaining = sched.ticks_remaining.saturating_sub(1);
                if sched.ticks_remaining == 0 {
                    ready.push(sched.function);
                } else {
                    remaining.push(sched);
                }
            }
        }
        self.scheduled = remaining;
        for func in ready {
            self.roots.push_back(ExecutionRoot::Scheduled(func));
        }
    }

    /// Returns commands encountered during execution that are not modeled.
    #[must_use]
    pub fn unsupported_commands(&self) -> &[String] {
        &self.unsupported_commands
    }

    fn execute_console_line(&mut self, command: &str, budget: &CommandBudget) -> CommandOutcome {
        let parsed = parse_command(command);
        self.execute_parsed(&parsed, budget)
    }

    fn execute_function_root(&mut self, name: &str, budget: &CommandBudget) -> CommandOutcome {
        let Some(body) = self.functions.get(name).cloned() else {
            let msg = format!("Unknown function: {name}");
            self.log.push(msg);
            return CommandOutcome::failure(vec![]);
        };
        let mut budget = budget.clone();
        let mut outcome = CommandOutcome::default();
        for cmd_text in &body.commands {
            if budget.sequence_remaining == 0 {
                break;
            }
            outcome = self.execute_console_line(cmd_text, &budget);
            if !outcome.continued {
                break;
            }
            budget.sequence_remaining = budget.sequence_remaining.saturating_sub(1);
        }
        outcome
    }

    fn execute_parsed(&mut self, cmd: &ParsedCommand, budget: &CommandBudget) -> CommandOutcome {
        match cmd {
            ParsedCommand::Scoreboard(sc) => self.execute_scoreboard(sc),
            ParsedCommand::Data(dc) => self.execute_data(dc),
            ParsedCommand::Execute(ec) => self.execute_execute(ec, budget),
            ParsedCommand::Function(fc) => self.execute_function_call(fc, budget),
            ParsedCommand::Say(msg) => self.execute_say(msg),
            ParsedCommand::Teleport(tc) => self.execute_teleport(tc),
            ParsedCommand::Return(rc) => self.execute_return(rc, budget),
            ParsedCommand::Gamerule(gc) => self.execute_gamerule(gc),
            ParsedCommand::Schedule(sc) => self.execute_schedule(sc),
            ParsedCommand::Summon(sc) => self.execute_summon(sc),
            ParsedCommand::Forceload(_) => CommandOutcome::success(0, vec![]),
            ParsedCommand::Setblock(cmd) => {
                self.world
                    .blocks
                    .set(cmd.x, cmd.y, cmd.z, &self.current_dimension, &cmd.block);
                CommandOutcome::success(0, vec![])
            }
            ParsedCommand::Kill(cmd) => {
                let count = self
                    .world
                    .entities
                    .remove_all_matching(&cmd.selector, &self.world.scoreboard);
                CommandOutcome::success(i32::try_from(count).unwrap_or(i32::MAX), vec![])
            }
            ParsedCommand::Tag(cmd) => {
                let ids = self
                    .world
                    .entities
                    .resolve_selector(&cmd.selector, &self.world.scoreboard);
                if ids.is_empty() {
                    return CommandOutcome::failure(vec![]);
                }
                let count = ids.len();
                match &cmd.action {
                    TagAction::Add => {
                        for &id in &ids {
                            self.world.entities.add_tag(id, &cmd.tag);
                        }
                        CommandOutcome::success(1, vec![])
                    }
                    TagAction::Remove => {
                        for &id in &ids {
                            self.world.entities.remove_tag(id, &cmd.tag);
                        }
                        CommandOutcome::success(1, vec![])
                    }
                    TagAction::List => {
                        if let Some(e) = self.world.entities.get(ids[0]) {
                            CommandOutcome::success(
                                i32::try_from(count).unwrap_or(i32::MAX),
                                vec![format!("Entity {} has tags: {}", ids[0], e.tags.join(", "))],
                            )
                        } else {
                            CommandOutcome::failure(vec![])
                        }
                    }
                }
            }
            ParsedCommand::Tellraw(cmd) => {
                self.log.push(format!("[CHAT] {}", cmd.message));
                CommandOutcome::success(1, vec![])
            }
            ParsedCommand::Title(cmd) => {
                self.log
                    .push(format!("[TITLE] {} {}", cmd.action, cmd.text));
                CommandOutcome::success(1, vec![])
            }
            ParsedCommand::Playsound => self.unsupported_command("playsound"),
            ParsedCommand::Rotate(cmd) => {
                let ids = self
                    .world
                    .entities
                    .resolve_selector(&cmd.selector, &self.world.scoreboard);
                let count = ids.len();
                for id in ids {
                    self.world.entities.set_rotation(id, cmd.yaw, cmd.pitch);
                }
                CommandOutcome::success(i32::try_from(count).unwrap_or(i32::MAX), vec![])
            }
            ParsedCommand::Item(command) => {
                self.unsupported_command(&format!("item {}", command.action))
            }
            ParsedCommand::Advancement(command) => {
                let ids = self
                    .world
                    .entities
                    .resolve_selector(&command.selector, &self.world.scoreboard);
                if ids.is_empty() {
                    return CommandOutcome::failure(vec![]);
                }
                let count = i32::try_from(ids.len()).unwrap_or(i32::MAX);
                match command.action.as_str() {
                    "grant" => {
                        for id in ids {
                            self.world
                                .entities
                                .grant_advancement(id, &command.advancement);
                        }
                        CommandOutcome::success(count, vec![])
                    }
                    "revoke" => {
                        for id in ids {
                            self.world
                                .entities
                                .revoke_advancement(id, &command.advancement);
                        }
                        CommandOutcome::success(count, vec![])
                    }
                    _ => CommandOutcome::failure(vec![]),
                }
            }
            ParsedCommand::Loot(command) => self.execute_loot(command),
            ParsedCommand::Reload => match self.load_datapacks() {
                Ok(()) => CommandOutcome::success(
                    0,
                    vec!["[Server thread/INFO]: Reloading ResourcePackManager".to_owned()],
                ),
                Err(error) => CommandOutcome::failure(vec![error]),
            },
            ParsedCommand::Stop => CommandOutcome::failure(vec![
                "[Server thread/INFO]: Stopping the server".to_owned(),
            ]),
            ParsedCommand::Raw(raw) => self.unsupported_command(raw),
        }
    }

    fn unsupported_command(&mut self, command: &str) -> CommandOutcome {
        self.unsupported_commands.push(command.to_owned());
        CommandOutcome::failure(vec![format!(
            "[mcfunction-executor/ERROR]: unsupported command: {command}"
        )])
    }

    // ── scoreboard ──────────────────────────────────────────────

    fn execute_scoreboard(&mut self, cmd: &super::parse::ScoreboardCmd) -> CommandOutcome {
        match &cmd.subcommand {
            ScoreboardSub::ObjectivesAdd { objective, .. } => {
                // Objectives are implicitly created; no-op for now
                self.log.push(format!(
                    "[Server thread/INFO]: Created new objective [{objective}]"
                ));
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersSet {
                holder,
                objective,
                value,
            } => {
                self.world.scoreboard.set(holder, objective, *value);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersAdd {
                holder,
                objective,
                amount,
            } => {
                self.world.scoreboard.add(holder, objective, *amount);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersRemove {
                holder,
                objective,
                amount,
            } => {
                self.world
                    .scoreboard
                    .remove_amount(holder, objective, *amount);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersGet { holder, objective } => {
                match self.world.scoreboard.get(holder, objective) {
                    Some(value) => {
                        let log = format!("{holder} has {value} [{objective}]");
                        CommandOutcome::success(value, vec![log])
                    }
                    None => {
                        let log = format!(
                            "[Server thread/INFO]: Can't get value of {objective} for {holder} (not set)"
                        );
                        CommandOutcome::failure(vec![log])
                    }
                }
            }
            ScoreboardSub::PlayersReset { holder, objective } => {
                self.world.scoreboard.reset(holder, objective);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersOperation {
                target_holder,
                target_objective,
                op,
                source_holder,
                source_objective,
            } => {
                let op = ScoreOp::from_token(op);
                if let Some(op) = op {
                    self.world.scoreboard.scoreboard_operation(
                        target_holder,
                        target_objective,
                        op,
                        source_holder,
                        source_objective,
                    );
                    CommandOutcome::success(0, vec![])
                } else {
                    CommandOutcome::failure(vec![])
                }
            }
        }
    }

    // ── data ────────────────────────────────────────────────────

    fn execute_data(&mut self, cmd: &super::parse::DataCmd) -> CommandOutcome {
        use super::parse::DataSub;
        match &cmd.subcommand {
            DataSub::Get {
                storage,
                path,
                scale,
            } => {
                // Handle "entity:@s" prefix for entity data queries
                if let Some(selector) = storage.strip_prefix("entity:") {
                    return self.execute_entity_data_get(selector, path, *scale);
                }
                if let Some(block_ref) = storage.strip_prefix("block:") {
                    let Some((x, y, z)) = resolve_block_coords(block_ref) else {
                        return CommandOutcome::failure(vec!["invalid block position".to_owned()]);
                    };
                    let Some(value) =
                        self.world
                            .blocks
                            .nbt_get(x, y, z, &self.current_dimension, path)
                    else {
                        return CommandOutcome::failure(vec![format!(
                            "Block {block_ref} has no data at {path}"
                        )]);
                    };
                    let snbt = value.to_snbt();
                    let result = match scale {
                        Some(scale) => ((value.as_i32() as f64) * scale) as i32,
                        None => value.as_i32(),
                    };
                    return CommandOutcome::success(
                        result,
                        vec![format!("{block_ref} has the following block data: {snbt}")],
                    );
                }
                let value = self.world.storage.get(storage, path);
                match value {
                    Some(val) => {
                        let snbt = val.to_snbt();
                        let log = format!("{storage} has the following contents: {snbt}");
                        let result = match scale {
                            Some(s) => ((val.as_i32() as f64) * s) as i32,
                            None => val.as_i32(),
                        };
                        CommandOutcome::success(result, vec![log])
                    }
                    None => {
                        let log =
                            format!("[Server thread/INFO]: Storage {storage} not found at {path}");
                        CommandOutcome::failure(vec![log])
                    }
                }
            }
            DataSub::Remove { storage, path } => {
                if let Some(sel) = storage.strip_prefix("entity:") {
                    let ids = self
                        .world
                        .entities
                        .resolve_selector(sel, &self.world.scoreboard);
                    if let Some(eid) = ids.first().copied() {
                        match self.world.entities.nbt_remove(eid, path) {
                            Ok(()) => return CommandOutcome::success(1, vec![]),
                            Err(e) => return CommandOutcome::failure(vec![e]),
                        }
                    }
                    return CommandOutcome::failure(vec![]);
                }
                if let Some(block_ref) = storage.strip_prefix("block:") {
                    let Some((x, y, z)) = resolve_block_coords(block_ref) else {
                        return CommandOutcome::failure(vec!["invalid block position".to_owned()]);
                    };
                    return match self.world.blocks.nbt_remove(
                        x,
                        y,
                        z,
                        &self.current_dimension,
                        path,
                    ) {
                        Ok(()) => CommandOutcome::success(1, vec![]),
                        Err(error) => CommandOutcome::failure(vec![error]),
                    };
                }
                let _ = self.world.storage.remove(storage, path);
                CommandOutcome::success(1, vec![])
            }
            DataSub::Modify {
                storage,
                path,
                mode,
                source,
            } => {
                if let Some(sel) = storage.strip_prefix("entity:") {
                    let ids = self
                        .world
                        .entities
                        .resolve_selector(sel, &self.world.scoreboard);
                    let Some(eid) = ids.first().copied() else {
                        return CommandOutcome::failure(vec![]);
                    };
                    let value = || {
                        if let Some(snbt) = source.strip_prefix("value ") {
                            NbtValue::from_snbt(snbt)
                        } else {
                            self.data_source_value(source)
                        }
                    };
                    let result = match mode.as_str() {
                        "set" => {
                            value().and_then(|value| self.world.entities.nbt_set(eid, path, value))
                        }
                        "append" => value()
                            .and_then(|value| self.world.entities.nbt_append(eid, path, value)),
                        "prepend" => value()
                            .and_then(|value| self.world.entities.nbt_prepend(eid, path, value)),
                        "merge" => value()
                            .and_then(|value| self.world.entities.nbt_merge(eid, path, value)),
                        _ => Err(format!("unknown mode: {mode}")),
                    };
                    return match result {
                        Ok(()) => CommandOutcome::success(1, vec![]),
                        Err(e) => CommandOutcome::failure(vec![e]),
                    };
                }
                if let Some(block_ref) = storage.strip_prefix("block:") {
                    let Some((x, y, z)) = resolve_block_coords(block_ref) else {
                        return CommandOutcome::failure(vec!["invalid block position".to_owned()]);
                    };
                    let result = match mode.as_str() {
                        "set" => {
                            let value = if let Some(snbt) = source.strip_prefix("value ") {
                                NbtValue::from_snbt(snbt)
                            } else {
                                self.data_source_value(source)
                            };
                            value.and_then(|value| {
                                self.world.blocks.nbt_set(
                                    x,
                                    y,
                                    z,
                                    &self.current_dimension,
                                    path,
                                    value,
                                )
                            })
                        }
                        "merge" => source
                            .strip_prefix("value ")
                            .ok_or_else(|| format!("expected value: {source}"))
                            .and_then(NbtValue::from_snbt)
                            .and_then(|value| {
                                self.world.blocks.nbt_merge(
                                    x,
                                    y,
                                    z,
                                    &self.current_dimension,
                                    path,
                                    value,
                                )
                            }),
                        _ => Err(format!("unsupported block data mode: {mode}")),
                    };
                    return match result {
                        Ok(()) => CommandOutcome::success(1, vec![]),
                        Err(error) => CommandOutcome::failure(vec![error]),
                    };
                }
                let result = match mode.as_str() {
                    "set" => self.data_modify_set(storage, path, source),
                    "append" => self.data_modify_append(storage, path, source),
                    "prepend" => self.data_modify_prepend(storage, path, source),
                    "merge" => self.data_modify_merge(storage, path, source),
                    "from" => self.data_modify_from(storage, path, source),
                    _ => Err(format!("unknown mode: {mode}")),
                };
                match result {
                    Ok(()) => CommandOutcome::success(1, vec![]),
                    Err(e) => {
                        self.log.push(format!("[ERROR] data modify: {e}"));
                        CommandOutcome::failure(vec![])
                    }
                }
            }
        }
    }

    fn execute_entity_data_get(
        &self,
        selector: &str,
        path: &str,
        scale: Option<f64>,
    ) -> CommandOutcome {
        let ids = self
            .world
            .entities
            .resolve_selector(selector, &self.world.scoreboard);
        let Some(entity_id) = ids.first().copied() else {
            return CommandOutcome::failure(vec!["No entity was found".to_owned()]);
        };
        let Some(value) = self.world.entities.nbt_get(entity_id, path) else {
            return CommandOutcome::failure(vec![format!("Entity {selector} not found at {path}")]);
        };
        let snbt = value.to_snbt();
        let result = match scale {
            Some(s) => ((value.as_i32() as f64) * s) as i32,
            None => value.as_i32(),
        };
        CommandOutcome::success(
            result,
            vec![format!("{selector} has the following entity data: {snbt}")],
        )
    }

    fn execute_loot(&mut self, command: &super::parse::LootCmd) -> CommandOutcome {
        if command.action == "spawn" {
            let Some((x, y, z)) = command.pos else {
                return CommandOutcome::failure(vec!["loot spawn requires a position".to_owned()]);
            };
            self.world.entities.summon("minecraft:item", x, y, z, &[]);
            return CommandOutcome::success(1, vec![]);
        }
        CommandOutcome::failure(vec![format!("unsupported loot action: {}", command.action)])
    }

    fn data_modify_set(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        if let Some(snbt) = source.strip_prefix("value ") {
            let val = NbtValue::from_snbt(snbt)?;
            self.world.storage.set(storage, path, val)
        } else {
            let value = self.data_source_value(source)?;
            self.world.storage.set(storage, path, value)
        }
    }

    fn data_modify_append(
        &mut self,
        storage: &str,
        path: &str,
        source: &str,
    ) -> Result<(), String> {
        if let Some(snbt) = source.strip_prefix("value ") {
            let val = NbtValue::from_snbt(snbt)?;
            self.world.storage.append(storage, path, val)
        } else {
            let value = self.data_source_value(source)?;
            self.world.storage.append(storage, path, value)
        }
    }

    fn data_modify_prepend(
        &mut self,
        storage: &str,
        path: &str,
        source: &str,
    ) -> Result<(), String> {
        if let Some(snbt) = source.strip_prefix("value ") {
            let val = NbtValue::from_snbt(snbt)?;
            self.world.storage.prepend(storage, path, val)
        } else {
            let value = self.data_source_value(source)?;
            self.world.storage.prepend(storage, path, value)
        }
    }

    fn data_modify_merge(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        let snbt = source
            .strip_prefix("value ")
            .ok_or_else(|| format!("expected value: {source}"))?;
        let val = NbtValue::from_snbt(snbt)?;
        self.world.storage.merge(storage, path, val)
    }

    fn data_modify_from(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        let value = self.data_source_value(source)?;
        self.world.storage.set(storage, path, value)
    }

    fn data_source_value(&self, source: &str) -> Result<NbtValue, String> {
        if let Some(from) = source.strip_prefix("from storage ") {
            let (storage, path) = split_first_word(from);
            return self
                .world
                .storage
                .get(storage, path)
                .ok_or_else(|| format!("source not found: storage {storage} {path}"));
        }
        if let Some(from) = source.strip_prefix("from entity ") {
            let (selector, path) = split_first_word(from);
            let entity = self
                .world
                .entities
                .resolve_selector(selector, &self.world.scoreboard)
                .into_iter()
                .next()
                .ok_or_else(|| format!("source entity not found: {selector}"))?;
            return self
                .world
                .entities
                .nbt_get(entity, path)
                .ok_or_else(|| format!("source not found: entity {selector} {path}"));
        }
        Err(format!(
            "expected `from storage` or `from entity`: {source}"
        ))
    }

    // ── execute ─────────────────────────────────────────────────

    fn execute_execute(&mut self, cmd: &ExecuteCmd, budget: &CommandBudget) -> CommandOutcome {
        let mut ctx = ExecutionContext {
            dimension: self.current_dimension.clone(),
            ..ExecutionContext::default()
        };
        let mut skip = false;

        for modifier in &cmd.modifiers {
            if skip {
                break;
            }
            match modifier {
                ExecuteModifier::As(selector) => {
                    if selector == "@s" {
                        // keep current executor
                    } else {
                        let mut ids = self
                            .world
                            .entities
                            .resolve_selector(selector, &self.world.scoreboard);
                        if ids.is_empty() {
                            skip = true;
                            continue;
                        }
                        let fork_limit = usize::try_from(budget.fork_limit).unwrap_or(usize::MAX);
                        if ids.len() > fork_limit {
                            return CommandOutcome::failure(vec![format!(
                                "Command fork limit exceeded: {} > {fork_limit}",
                                ids.len()
                            )]);
                        }
                        ctx.executor = Some(ids[0]);
                        ctx.entity_ids = std::mem::take(&mut ids);
                    }
                }
                ExecuteModifier::At(selector) => {
                    if selector == "@s" {
                        if let Some(id) = ctx.executor {
                            if let Some(e) = self.world.entities.get(id) {
                                ctx.x = e.x;
                                ctx.y = e.y;
                                ctx.z = e.z;
                                ctx.yaw = e.yaw;
                                ctx.pitch = e.pitch;
                                ctx.dimension = e.dimension.clone();
                            }
                        }
                    } else {
                        let ids = self
                            .world
                            .entities
                            .resolve_selector(selector, &self.world.scoreboard);
                        if let Some(id) = ids.first() {
                            if let Some(e) = self.world.entities.get(*id) {
                                ctx.x = e.x;
                                ctx.y = e.y;
                                ctx.z = e.z;
                                ctx.yaw = e.yaw;
                                ctx.pitch = e.pitch;
                                ctx.dimension = e.dimension.clone();
                            }
                        }
                    }
                }
                ExecuteModifier::In(dim) => {
                    // In 26.2, `in` does NOT scale coordinates (tests prove this).
                    // It just sets the dimension for subsequent operations.
                    ctx.dimension = dim.clone();
                }
                ExecuteModifier::Positioned(pos) => {
                    apply_positioned(&mut ctx, pos);
                }
                ExecuteModifier::Rotated(rot) => {
                    apply_rotated(&mut ctx, rot);
                }
                ExecuteModifier::Anchored(a) => {
                    ctx.anchor = match a.as_str() {
                        "eyes" => Anchor::Eyes,
                        _ => Anchor::Feet,
                    };
                }
                ExecuteModifier::Align(axes) => {
                    for c in axes.chars() {
                        match c {
                            'x' => ctx.x = ctx.x.floor(),
                            'y' => ctx.y = ctx.y.floor(),
                            'z' => ctx.z = ctx.z.floor(),
                            _ => {}
                        }
                    }
                }
                ExecuteModifier::If(cond) => {
                    if !self.evaluate_condition(cond, &ctx) {
                        skip = true;
                    }
                }
                ExecuteModifier::Unless(cond) => {
                    if self.evaluate_condition(cond, &ctx) {
                        skip = true;
                    }
                }
                ExecuteModifier::StoreSuccess(_) | ExecuteModifier::StoreResult(_) => {}
            }
        }

        if skip {
            return CommandOutcome::failure(vec![]);
        }

        // Inject context into nested command execution
        let outcome = self.execute_nested_with_context(&cmd.nested, &ctx, budget);

        // Handle store captures
        for modifier in &cmd.modifiers {
            match modifier {
                ExecuteModifier::StoreSuccess(dest) => {
                    self.store_outcome(dest, i32::from(outcome.success));
                }
                ExecuteModifier::StoreResult(dest) => {
                    self.store_outcome(dest, outcome.result);
                }
                _ => {}
            }
        }
        outcome
    }

    fn execute_nested_with_context(
        &mut self,
        cmd: &ParsedCommand,
        ctx: &ExecutionContext,
        budget: &CommandBudget,
    ) -> CommandOutcome {
        match cmd {
            ParsedCommand::Teleport(tc) => {
                self.execute_teleport_at(ctx, &tc.target, &tc.destination)
            }
            ParsedCommand::Say(msg) => {
                let log = if let Some(id) = ctx.executor {
                    format!("[entity_{id}] {msg}")
                } else {
                    self.execute_say(msg)
                        .log
                        .first()
                        .cloned()
                        .unwrap_or_default()
                };
                CommandOutcome::success(0, vec![log])
            }
            _ => {
                let previous_dimension =
                    std::mem::replace(&mut self.current_dimension, ctx.dimension.clone());
                let outcome = self.execute_parsed(cmd, budget);
                self.current_dimension = previous_dimension;
                outcome
            }
        }
    }

    // ── function ────────────────────────────────────────────────

    fn execute_function_call(
        &mut self,
        cmd: &super::parse::FunctionCmd,
        budget: &CommandBudget,
    ) -> CommandOutcome {
        let name = if cmd.is_tag {
            format!("#{}", cmd.name)
        } else {
            cmd.name.clone()
        };

        if cmd.is_tag {
            if let Some(entries) = self.function_tags.get(&cmd.name).cloned() {
                let mut outcome = CommandOutcome::success(0, vec![]);
                for entry in entries {
                    let mut sub_budget = budget.clone();
                    let entry_outcome = self.execute_function_body(&entry, &mut sub_budget, None);
                    outcome.result = entry_outcome.result;
                    outcome.success = entry_outcome.success;
                    outcome.log.extend(entry_outcome.log);
                    if !entry_outcome.continued {
                        outcome.continued = false;
                        break;
                    }
                }
                return outcome;
            }
        }

        let macro_args = if let Some((storage, path)) = &cmd.with_storage {
            self.world.storage.get(storage, path)
        } else if let Some(snbt) = &cmd.inline_args {
            NbtValue::from_snbt(snbt).ok()
        } else {
            None
        };

        if (cmd.with_storage.is_some() || cmd.inline_args.is_some()) && macro_args.is_none() {
            return CommandOutcome::failure(vec!["Function macro arguments were not found".into()]);
        }

        self.execute_function_body(&name, &mut budget.clone(), macro_args.as_ref())
    }

    fn execute_function_body(
        &mut self,
        name: &str,
        budget: &mut CommandBudget,
        macro_args: Option<&NbtValue>,
    ) -> CommandOutcome {
        // If the entry is a tag reference (starts with #), dispatch through tag logic.
        if let Some(tag_name) = name.strip_prefix('#') {
            let fc = crate::parse::FunctionCmd {
                name: tag_name.to_owned(),
                is_tag: true,
                with_storage: None,
                inline_args: None,
            };
            return self.execute_function_call(&fc, budget);
        }

        let Some(body) = self.functions.get(name).cloned() else {
            // Vanilla silently skips optional/missing tag entries
            // For non-tag lookups this means the condition failed
            return CommandOutcome::failure(vec![]);
        };

        let mut outcome = CommandOutcome::success(0, vec![]);
        for cmd_text in &body.commands {
            if budget.sequence_remaining == 0 {
                outcome.continued = false;
                break;
            }
            budget.sequence_remaining = budget.sequence_remaining.saturating_sub(1);
            let expanded;
            let command = if let Some(macro_command) = cmd_text.strip_prefix('$') {
                let Some(arguments) = macro_args else {
                    return CommandOutcome::failure(vec![
                        "Macro function called without arguments".into(),
                    ]);
                };
                match expand_macro_command(macro_command, arguments) {
                    Ok(command) => {
                        expanded = command;
                        expanded.as_str()
                    }
                    Err(error) => return CommandOutcome::failure(vec![error]),
                }
            } else {
                cmd_text.as_str()
            };
            let parsed = parse_command(command);
            let cmd_outcome = match &parsed {
                ParsedCommand::Return(rc) => self.execute_return(rc, budget),
                other => self.execute_parsed(other, budget),
            };
            outcome.success = cmd_outcome.success;
            outcome.result = cmd_outcome.result;
            outcome.log.extend(cmd_outcome.log);
            if !cmd_outcome.continued {
                outcome.continued = false;
                break;
            }
        }
        outcome
    }

    // ── say ─────────────────────────────────────────────────────

    fn execute_say(&mut self, msg: &str) -> CommandOutcome {
        let log = format!("[Server] {msg}");
        CommandOutcome::success(0, vec![log])
    }

    // ── teleport ────────────────────────────────────────────────

    fn execute_teleport(&mut self, cmd: &super::parse::TeleportCmd) -> CommandOutcome {
        let context = ExecutionContext {
            dimension: self.current_dimension.clone(),
            ..ExecutionContext::default()
        };
        self.execute_teleport_at(&context, &cmd.target, &cmd.destination)
    }

    // ── return ──────────────────────────────────────────────────

    fn execute_return(&mut self, cmd: &ReturnCmd, budget: &CommandBudget) -> CommandOutcome {
        match cmd {
            ReturnCmd::Value(n) => CommandOutcome::returned(*n, vec![]),
            ReturnCmd::Fail => CommandOutcome::returned_fail(vec![]),
            ReturnCmd::Run(nested) => {
                let outcome = self.execute_parsed(nested, budget);
                if outcome.success == 0 {
                    CommandOutcome::returned_fail(outcome.log)
                } else {
                    CommandOutcome::returned(outcome.result, outcome.log)
                }
            }
        }
    }

    // ── gamerule ────────────────────────────────────────────────

    fn execute_gamerule(&mut self, cmd: &super::parse::GameruleCmd) -> CommandOutcome {
        self.world.gamerules.set(&cmd.rule, cmd.value);
        let log = format!(
            "[Server thread/INFO]: Gamerule {} is now set to: {}",
            cmd.rule, cmd.value
        );
        CommandOutcome::success(cmd.value, vec![log])
    }

    // ── schedule ────────────────────────────────────────────────

    fn execute_schedule(&mut self, cmd: &ScheduleCmd) -> CommandOutcome {
        if cmd.replace {
            self.scheduled.retain(|s| s.function != cmd.function);
        }
        self.scheduled.push(PendingSchedule {
            ticks_remaining: cmd.delay_ticks,
            function: cmd.function.clone(),
        });
        CommandOutcome::success(0, vec![])
    }

    // ── summon ──────────────────────────────────────────────────

    fn execute_summon(&mut self, cmd: &super::parse::SummonCmd) -> CommandOutcome {
        let nbt = match cmd.nbt.as_deref() {
            Some(snbt) => match NbtValue::from_snbt(snbt) {
                Ok(nbt) => nbt,
                Err(error) => return CommandOutcome::failure(vec![error]),
            },
            None => NbtValue::compound(vec![]),
        };
        let tags = match &nbt {
            NbtValue::Compound(entries) => entries
                .iter()
                .find(|(key, _)| key == "Tags")
                .and_then(|(_, value)| match value {
                    NbtValue::List(tags) => Some(
                        tags.iter()
                            .filter_map(|tag| match tag {
                                NbtValue::String(tag) => Some(tag.clone()),
                                _ => None,
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let rotation = match &nbt {
            NbtValue::Compound(entries) => entries
                .iter()
                .find(|(key, _)| key == "Rotation")
                .and_then(|(_, value)| match value {
                    NbtValue::List(rotation) if rotation.len() >= 2 => {
                        Some((rotation[0].as_f32(), rotation[1].as_f32()))
                    }
                    _ => None,
                }),
            _ => None,
        };
        let x = cmd.pos.map_or(0.0, |(x, _, _)| x);
        let y = cmd.pos.map_or(0.0, |(_, y, _)| y);
        let z = cmd.pos.map_or(0.0, |(_, _, z)| z);
        let id = self
            .world
            .entities
            .summon_with_nbt(&cmd.entity_type, x, y, z, &tags, nbt);
        if let Some((yaw, pitch)) = rotation {
            self.world.entities.set_rotation(id, yaw, pitch);
        }
        let type_name = cmd
            .entity_type
            .strip_prefix("minecraft:")
            .unwrap_or(&cmd.entity_type)
            .replace('_', " ");
        let name = capitalize(&type_name);
        let log = format!("[Server thread/INFO]: Summoned new {name}");
        CommandOutcome::success(0, vec![log])
    }
}

#[derive(Clone, Debug)]
struct ExecutionContext {
    executor: Option<u64>,
    entity_ids: Vec<u64>,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    dimension: String,
    anchor: Anchor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Anchor {
    Feet,
    Eyes,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            executor: None,
            entity_ids: vec![],
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            dimension: "minecraft:overworld".into(),
            anchor: Anchor::Feet,
        }
    }
}

fn evaluate_score_range(value: Option<i32>, range: &str) -> bool {
    let Some(value) = value else {
        return false;
    };
    // Exact match
    if let Ok(n) = range.parse::<i32>() {
        return value == n;
    }
    // N.. (at least N)
    if let Some(rest) = range.strip_suffix("..") {
        if let Ok(n) = rest.parse::<i32>() {
            return value >= n;
        }
    }
    // ..N (at most N)
    if let Some(rest) = range.strip_prefix("..") {
        if let Ok(n) = rest.parse::<i32>() {
            return value <= n;
        }
    }
    // N..M (between)
    if let Some(dot_pos) = range.find("..") {
        let min = range[..dot_pos].parse::<i32>().unwrap_or(i32::MIN);
        let max = range[dot_pos + 2..].parse::<i32>().unwrap_or(i32::MAX);
        return value >= min && value <= max;
    }
    false
}

// ── spatial helpers ─────────────────────────────────────────

fn apply_positioned(ctx: &mut ExecutionContext, pos: &str) {
    let parts: Vec<&str> = pos.split_whitespace().collect();
    if parts.len() >= 3 {
        apply_world_axis(&mut ctx.x, parts[0]);
        apply_world_axis(&mut ctx.y, parts[1]);
        apply_world_axis(&mut ctx.z, parts[2]);
    }
}

fn apply_world_axis(current: &mut f64, spec: &str) {
    if spec.starts_with('^') {
        return; // local coords handled separately
    }
    if let Some(rel) = spec.strip_prefix('~') {
        if rel.is_empty() {
            // ~ → current position stays
        } else if let Ok(delta) = rel.parse::<f64>() {
            *current += delta;
        }
    } else if let Ok(abs) = spec.parse::<f64>() {
        *current = abs;
    }
}

fn apply_rotated(ctx: &mut ExecutionContext, rot: &str) {
    let parts: Vec<&str> = rot.split_whitespace().collect();
    if !parts.is_empty() {
        apply_rotation(&mut ctx.yaw, parts[0]);
        if parts.len() >= 2 {
            apply_rotation(&mut ctx.pitch, parts[1]);
        }
    }
}

fn apply_rotation(current: &mut f32, spec: &str) {
    if let Some(rel) = spec.strip_prefix('~') {
        if !rel.is_empty() {
            if let Ok(delta) = rel.parse::<f32>() {
                *current += delta;
            }
        }
    } else if let Ok(abs) = spec.parse::<f32>() {
        *current = abs;
    }
}

impl Executor {
    fn execute_teleport_at(
        &mut self,
        ctx: &ExecutionContext,
        target: &str,
        dest: &str,
    ) -> CommandOutcome {
        let ids = if target == "@s" {
            ctx.executor.map(|id| vec![id]).unwrap_or_default()
        } else {
            self.world
                .entities
                .resolve_selector(target, &self.world.scoreboard)
        };
        if ids.is_empty() {
            return CommandOutcome::failure(vec![]);
        }
        let parts: Vec<&str> = dest.split_whitespace().collect();
        let (mut new_x, mut new_y, mut new_z, mut new_yaw, mut new_pitch, dimension) =
            if parts.len() >= 3 {
                (
                    ctx.x,
                    ctx.y,
                    ctx.z,
                    ctx.yaw,
                    ctx.pitch,
                    ctx.dimension.clone(),
                )
            } else {
                let destination = self
                    .world
                    .entities
                    .resolve_selector(dest, &self.world.scoreboard)
                    .into_iter()
                    .next()
                    .and_then(|id| self.world.entities.get(id));
                let Some(destination) = destination else {
                    return CommandOutcome::failure(vec![]);
                };
                (
                    destination.x,
                    destination.y,
                    destination.z,
                    destination.yaw,
                    destination.pitch,
                    destination.dimension.clone(),
                )
            };
        if parts.len() >= 3 {
            apply_world_axis(&mut new_x, parts[0]);
            apply_world_axis(&mut new_y, parts[1]);
            apply_world_axis(&mut new_z, parts[2]);
            if parts.len() >= 5 {
                apply_rotation(&mut new_yaw, parts[3]);
                apply_rotation(&mut new_pitch, parts[4]);
            }
        }

        let count = i32::try_from(ids.len()).unwrap_or(i32::MAX);
        for id in &ids {
            self.world
                .entities
                .teleport_absolute(*id, new_x, new_y, new_z, new_yaw, new_pitch);
            self.world.entities.set_dimension(*id, &dimension);
        }
        CommandOutcome::success(count, vec![])
    }

    fn evaluate_condition(&mut self, cond: &ExecuteCondition, ctx: &ExecutionContext) -> bool {
        match cond {
            ExecuteCondition::Score {
                holder,
                objective,
                range,
            } => {
                let value = self.world.scoreboard.get(holder, objective);
                evaluate_score_range(value, range)
            }
            ExecuteCondition::ScoreCompare {
                left_holder,
                left_objective,
                op,
                right_holder,
                right_objective,
            } => {
                let Some(left) = self.world.scoreboard.get(left_holder, left_objective) else {
                    return false;
                };
                let Some(right) = self.world.scoreboard.get(right_holder, right_objective) else {
                    return false;
                };
                match op.as_str() {
                    "=" => left == right,
                    "<" => left < right,
                    "<=" => left <= right,
                    ">" => left > right,
                    ">=" => left >= right,
                    _ => false,
                }
            }
            ExecuteCondition::Data(storage, path) => {
                if path.starts_with('{') {
                    return evaluate_compound_predicate(self, storage, path);
                }
                if let Some(sel) = storage.strip_prefix("entity:") {
                    let ids = self
                        .world
                        .entities
                        .resolve_selector(sel, &self.world.scoreboard);
                    return ids
                        .first()
                        .map_or(false, |id| self.world.entities.nbt_exists(*id, path));
                }
                if let Some(block_ref) = storage.strip_prefix("block:") {
                    return resolve_block_coords(block_ref.trim()).map_or(false, |(x, y, z)| {
                        self.world.blocks.nbt_exists(x, y, z, &ctx.dimension, path)
                    });
                }
                self.world.storage.exists(storage, path)
            }
            ExecuteCondition::Entity(selector) => {
                self.world
                    .entities
                    .resolve_selector_at(selector, ctx.x, ctx.y, ctx.z, &self.world.scoreboard)
                    .len()
                    > 0
            }
            ExecuteCondition::Function(name) => {
                // Vanilla: execute if function RUNS the function and succeeds
                // when the function completed successfully (success=1, no return fail).
                let fc = crate::parse::FunctionCmd {
                    name: name.clone(),
                    is_tag: false,
                    with_storage: None,
                    inline_args: None,
                };
                let outcome = self.execute_function_call(
                    &fc,
                    &CommandBudget {
                        sequence_remaining: u32::MAX,
                        fork_limit: u32::MAX,
                    },
                );
                outcome.success == 1
            }
            ExecuteCondition::Block { x, y, z, block } => {
                self.world.blocks.matches(*x, *y, *z, &ctx.dimension, block)
            }
        }
    }

    fn store_outcome(&mut self, dest: &str, value: i32) {
        let words: Vec<&str> = dest.split_whitespace().collect();
        if words.len() >= 3 && words[0] == "score" {
            let holder = words[1].to_owned();
            let objective = words[2].to_owned();
            self.world.scoreboard.set(&holder, &objective, value);
        } else if words.len() >= 4 && words[0] == "storage" {
            let storage = words[1].to_owned();
            let path = words[2].to_owned();
            let numeric_type = words.get(3).map_or("int", |s| *s);
            let scale: f64 = words.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let _ = self
                .world
                .storage
                .store_numeric(&storage, &path, numeric_type, scale, value);
        } else if words.len() >= 3 && words[0] == "entity" {
            // execute store result entity @s Pos[0] double 0.001
            let selector = &words[1];
            let path = &words[2];
            let numeric_type = words.get(3).map_or("double", |s| *s);
            let scale: f64 = words.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let scaled = (value as f64) * scale;
            let ids = self
                .world
                .entities
                .resolve_selector(selector, &self.world.scoreboard);
            for id in ids {
                if matches!(&**path, "Pos[0]" | "Pos[1]" | "Pos[2]") {
                    let Some(e) = self.world.entities.get(id) else {
                        continue;
                    };
                    let (nx, ny, nz) = match &**path {
                        "Pos[0]" => (scaled, e.y, e.z),
                        "Pos[1]" => (e.x, scaled, e.z),
                        "Pos[2]" => (e.x, e.y, scaled),
                        _ => unreachable!(),
                    };
                    self.world
                        .entities
                        .teleport_absolute(id, nx, ny, nz, e.yaw, e.pitch);
                } else if let Ok(nbt) = NbtValue::from_scaled_i32(value, numeric_type, scale) {
                    let _ = self.world.entities.nbt_set(id, path, nbt);
                }
            }
        }
    }
}

fn split_first_word(input: &str) -> (&str, &str) {
    let input = input.trim();
    match input.find(char::is_whitespace) {
        Some(pos) => (&input[..pos], input[pos..].trim()),
        None => (input, ""),
    }
}

fn sorted_directory_paths(directory: &Path, label: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(directory)
        .map_err(|error| format!("read {label}: {error}"))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("read {label} entry: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
    }
}

fn expand_macro_command(command: &str, arguments: &NbtValue) -> Result<String, String> {
    let mut expanded = String::with_capacity(command.len());
    let mut remaining = command;

    while let Some(start) = remaining.find("$(") {
        expanded.push_str(&remaining[..start]);
        let placeholder = &remaining[start + 2..];
        let Some(end) = placeholder.find(')') else {
            return Err(format!("Unclosed macro placeholder in `{command}`"));
        };
        let path = &placeholder[..end];
        let operations = NbtValue::parse_path(path)?;
        let value = arguments
            .resolve(&operations)
            .ok_or_else(|| format!("Macro argument `{path}` was not found"))?;
        match value {
            NbtValue::String(value) => expanded.push_str(value),
            value => expanded.push_str(&value.to_snbt()),
        }
        remaining = &placeholder[end + 1..];
    }

    expanded.push_str(remaining);
    Ok(expanded)
}

fn evaluate_compound_predicate(exec: &Executor, storage: &str, compound_snbt: &str) -> bool {
    let Ok(predicate) = NbtValue::from_snbt(compound_snbt) else {
        return false;
    };
    let target = if let Some(sel) = storage.strip_prefix("entity:") {
        let ids = exec
            .world
            .entities
            .resolve_selector(sel, &exec.world.scoreboard);
        ids.first()
            .copied()
            .and_then(|id| exec.world.entities.nbt_get(id, ""))
    } else {
        exec.world.storage.get_root(storage).cloned()
    };
    target.map_or(false, |t| nbt_compound_subset(&t, &predicate))
}

fn nbt_compound_subset(target: &NbtValue, predicate: &NbtValue) -> bool {
    match (target, predicate) {
        (NbtValue::Compound(t), NbtValue::Compound(p)) => p.iter().all(|(pk, pv)| {
            t.iter()
                .find(|(tk, _)| tk == pk)
                .map(|(_, tv)| nbt_compound_subset(tv, pv))
                .unwrap_or(false)
        }),
        (NbtValue::List(t), NbtValue::List(p)) => {
            t.len() == p.len()
                && t.iter()
                    .zip(p.iter())
                    .all(|(a, b)| nbt_compound_subset(a, b))
        }
        _ => target == predicate,
    }
}

fn resolve_block_coords(block_ref: &str) -> Option<(i32, i32, i32)> {
    let parts: Vec<&str> = block_ref.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    fn rc(s: &str) -> Option<i32> {
        if let Some(r) = s.strip_prefix('~') {
            if r.is_empty() {
                Some(0)
            } else {
                r.parse().ok()
            }
        } else {
            s.parse().ok()
        }
    }
    Some((rc(parts[0])?, rc(parts[1])?, rc(parts[2])?))
}
