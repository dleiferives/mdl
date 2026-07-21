use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::parse::{
    parse_command, CommandOutcome, ExecuteCmd, ExecuteCondition, ExecuteModifier, ParsedCommand,
    ReturnCmd, ScheduleCmd, ScoreboardSub, TagAction,
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
        let _default_seq = version.default_max_command_sequence_length;
        let _default_forks = version.default_max_command_forks;
        Self {
            world: World::new(version),
            functions: HashMap::new(),
            function_tags: HashMap::new(),
            roots: VecDeque::new(),
            scheduled: Vec::new(),
            log: Vec::new(),
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
        for pack_entry in fs::read_dir(&datapacks_dir).map_err(|e| format!("read datapacks: {e}"))? {
            let pack_dir = pack_entry.map_err(|e| format!("read pack entry: {e}"))?.path();
            if !pack_dir.is_dir() {
                continue;
            }
            self.load_datapack(&pack_dir)?;
        }
        self.log.push("[Server thread/INFO]: Loaded datapacks".to_owned());

        // Execute minecraft:load tag
        if self.function_tags.contains_key("minecraft:load") {
            self.roots.push_back(ExecutionRoot::Console("function #minecraft:load".to_owned()));
            self.run_tick_loop()?;
        }
        Ok(())
    }

    fn load_datapack(&mut self, pack_dir: &PathBuf) -> Result<(), String> {
        let data_dir = pack_dir.join("data");
        if !data_dir.is_dir() {
            return Ok(());
        }
        for ns_entry in fs::read_dir(&data_dir).map_err(|e| format!("read namespace: {e}"))? {
            let ns_dir = ns_entry.map_err(|e| format!("read ns entry: {e}"))?.path();
            if !ns_dir.is_dir() {
                continue;
            }
            let namespace = ns_dir.file_name()
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

    fn load_functions(
        &mut self,
        namespace: &str,
        dir: &PathBuf,
        prefix: &str,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("read function dir: {e}"))? {
            let path = entry.map_err(|e| format!("read function entry: {e}"))?.path();
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let resource = if prefix.is_empty() {
                format!("{namespace}:{name}")
            } else {
                format!("{namespace}:{prefix}/{name}")
            };

            if path.is_dir() {
                let new_prefix = if prefix.is_empty() { name.to_owned() } else { format!("{prefix}/{name}") };
                self.load_functions(namespace, &path, &new_prefix)?;
            } else if path.extension().map_or(false, |ext| ext == "mcfunction") {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("read function {resource}: {e}"))?;
                let commands: Vec<String> = content
                    .lines()
                    .map(|l| l.trim().to_owned())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .collect();
                self.functions.insert(resource.clone(), FunctionBody { commands });
            }
        }
        Ok(())
    }

    fn load_function_tags(&mut self, namespace: &str, dir: &PathBuf) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("read tag dir: {e}"))? {
            let path = entry.map_err(|e| format!("read tag entry: {e}"))?.path();
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let resource = format!("{namespace}:{name}");

            if path.extension().map_or(false, |ext| ext == "json") {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("read tag {resource}: {e}"))?;
                if let Ok(tag) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(values) = tag.get("values").and_then(|v| v.as_array()) {
                        let entries: Vec<String> = values.iter().filter_map(|v| {
                            match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Object(obj) => {
                                    obj.get("id").and_then(|id| id.as_str()).map(String::from)
                                }
                                _ => None,
                            }
                        }).collect();
                        self.function_tags.insert(resource, entries);
                    }
                }
            }
        }
        Ok(())
    }

    /// Enqueues a console command as a root for the current tick and runs
    /// the tick loop until all work is done.
    pub fn console_command(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.roots.push_back(ExecutionRoot::Console(command.to_owned()));
        self.run_tick_loop()?;
        Ok(std::mem::take(&mut self.log))
    }

    /// Advances the world by the given number of ticks, processing all roots.
    pub fn advance_ticks(&mut self, ticks: u64) -> Result<(), String> {
        for _ in 0..ticks {
            self.world.tick += 1;
            self.process_tick()?;
        }
        Ok(())
    }

    fn run_tick_loop(&mut self) -> Result<(), String> {
        loop {
            // Process all ready roots for the current tick
            if !self.roots.is_empty() {
                self.process_tick()?;
                continue;
            }
            // Advance schedules to next tick
            if !self.scheduled.is_empty() {
                self.world.tick += 1;
                let mut ready = Vec::new();
                let mut remaining = Vec::new();
                for mut sched in self.scheduled.drain(..) {
                    if sched.ticks_remaining == 0 {
                        ready.push(sched.function);
                    } else {
                        sched.ticks_remaining -= 1;
                        remaining.push(sched);
                    }
                }
                self.scheduled = remaining;
                for func in ready {
                    self.roots.push_back(ExecutionRoot::Scheduled(func));
                }
                continue;
            }
            break;
        }
        Ok(())
    }

    fn process_tick(&mut self) -> Result<(), String> {
        let roots: Vec<ExecutionRoot> = self.roots.drain(..).collect();
        for root in roots {
            let budget = CommandBudget {
                sequence_remaining: self.world.gamerules.rules.get("max_command_sequence_length").copied().unwrap_or(65536) as u32,
                fork_limit: self.world.gamerules.rules.get("max_command_forks").copied().unwrap_or(65536) as u32,
            };
            let outcome = match root {
                ExecutionRoot::Console(cmd) => self.execute_console_line(&cmd, &budget),
                ExecutionRoot::Scheduled(func) => self.execute_function_root(&func, &budget),
            };
            self.log.extend(outcome.log);
        }
        Ok(())
    }

    fn has_pending_schedules(&self) -> bool {
        !self.scheduled.is_empty()
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
                self.world.blocks.set(cmd.x, cmd.y, cmd.z, &self.current_dimension, &cmd.block);
                CommandOutcome::success(0, vec![])
            }
            ParsedCommand::Kill(cmd) => {
                let count = self.world.entities.remove_all_matching(&cmd.selector, &self.world.scoreboard);
                CommandOutcome::success(count as i32, vec![])
            }
            ParsedCommand::Tag(cmd) => {
                let ids = self.world.entities.resolve_selector(&cmd.selector, &self.world.scoreboard);
                if ids.is_empty() { return CommandOutcome::failure(vec![]); }
                let count = ids.len();
                match &cmd.action {
                    TagAction::Add => { for &id in &ids { self.world.entities.add_tag(id, &cmd.tag); } CommandOutcome::success(1, vec![]) }
                    TagAction::Remove => { for &id in &ids { self.world.entities.remove_tag(id, &cmd.tag); } CommandOutcome::success(1, vec![]) }
                    TagAction::List => {
                        if let Some(e) = self.world.entities.get(ids[0]) {
                            CommandOutcome::success(count as i32, vec![format!("Entity {} has tags: {}", ids[0], e.tags.join(", "))])
                        } else { CommandOutcome::failure(vec![]) }
                    }
                }
            }
            ParsedCommand::Tellraw(cmd) => {
                self.log.push(format!("[CHAT] {}", cmd.message));
                CommandOutcome::success(1, vec![])
            }
            ParsedCommand::Title(cmd) => {
                self.log.push(format!("[TITLE] {} {}", cmd.action, cmd.text));
                CommandOutcome::success(1, vec![])
            }
            ParsedCommand::Playsound => unimplemented!("playsound"),
            ParsedCommand::Rotate(cmd) => {
                let ids = self.world.entities.resolve_selector(&cmd.selector, &self.world.scoreboard);
                let count = ids.len();
                for id in ids { self.world.entities.set_rotation(id, cmd.yaw, cmd.pitch); }
                CommandOutcome::success(count as i32, vec![])
            }
            ParsedCommand::Item(_) => CommandOutcome::success(0, vec![]),
            ParsedCommand::Advancement(_cmd) => {
                let ids = self.world.entities.resolve_selector(&_cmd.selector, &self.world.scoreboard);
                if ids.is_empty() { return CommandOutcome::failure(vec![]); }
                match _cmd.action.as_str() {
                    "grant" => { self.world.entities.grant_advancement(ids[0], &_cmd.advancement); CommandOutcome::success(1, vec![]) }
                    "revoke" => { self.world.entities.revoke_advancement(ids[0], &_cmd.advancement); CommandOutcome::success(1, vec![]) }
                    _ => CommandOutcome::failure(vec![]),
                }
            }
            ParsedCommand::Loot(_) => CommandOutcome::success(1, vec![]),
            ParsedCommand::Reload => {
                let _ = self.load_datapacks();
                CommandOutcome::success(0, vec!["[Server thread/INFO]: Reloading ResourcePackManager".to_owned()])
            }
            ParsedCommand::Stop => CommandOutcome::failure(vec!["[Server thread/INFO]: Stopping the server".to_owned()]),
            ParsedCommand::Raw(raw) => {
                self.log.push(format!("[Server thread/INFO]: raw: {raw}"));
                CommandOutcome::success(0, vec![])
            }
        }
    }

    // ── scoreboard ──────────────────────────────────────────────

    fn execute_scoreboard(&mut self, cmd: &super::parse::ScoreboardCmd) -> CommandOutcome {
        match &cmd.subcommand {
            ScoreboardSub::ObjectivesAdd { objective, .. } => {
                // Objectives are implicitly created; no-op for now
                self.log.push(format!("[Server thread/INFO]: Created new objective [{objective}]"));
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersSet { holder, objective, value } => {
                self.world.scoreboard.set(holder, objective, *value);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersAdd { holder, objective, amount } => {
                self.world.scoreboard.add(holder, objective, *amount);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersRemove { holder, objective, amount } => {
                self.world.scoreboard.remove_amount(holder, objective, *amount);
                CommandOutcome::success(0, vec![])
            }
            ScoreboardSub::PlayersGet { holder, objective } => {
                match self.world.scoreboard.get(holder, objective) {
                    Some(value) => {
                        let log = format!("{holder} has {value} [{objective}]");
                        CommandOutcome::success(value, vec![log])
                    }
                    None => {
                        let log = format!("[Server thread/INFO]: Can't get value of {objective} for {holder} (not set)");
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
                        target_holder, target_objective,
                        op,
                        source_holder, source_objective,
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
            DataSub::Get { storage, path, scale } => {
                // Handle "entity:@s" prefix for entity data queries
                if let Some(selector) = storage.strip_prefix("entity:") {
                    return self.execute_entity_data_get(selector, path, *scale);
                }
                // Handle "block:" prefix (stubbed)
                if storage.starts_with("block:") {
                    return CommandOutcome::success(0, vec![]);
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
                        let log = format!("[Server thread/INFO]: Storage {storage} not found at {path}");
                        CommandOutcome::failure(vec![log])
                    }
                }
            }
            DataSub::Remove { storage, path } => {
                if let Some(sel) = storage.strip_prefix("entity:") {
                    let ids = self.world.entities.resolve_selector(sel, &self.world.scoreboard);
                    if let Some(eid) = ids.first().copied() {
                        match self.world.entities.nbt_remove(eid, path) {
                            Ok(()) => return CommandOutcome::success(1, vec![]),
                            Err(e) => return CommandOutcome::failure(vec![e]),
                        }
                    }
                    return CommandOutcome::failure(vec![]);
                }
                let _ = self.world.storage.remove(storage, path);
                CommandOutcome::success(1, vec![])
            }
            DataSub::Modify { storage, path, mode, source } => {
                if let Some(sel) = storage.strip_prefix("entity:") {
                    let ids = self.world.entities.resolve_selector(sel, &self.world.scoreboard);
                    let Some(eid) = ids.first().copied() else { return CommandOutcome::failure(vec![]); };
                    let result = match mode.as_str() {
                        "set" => if let Some(snbt) = source.strip_prefix("value ") {
                            NbtValue::from_snbt(snbt).and_then(|v| self.world.entities.nbt_set(eid, path, v))
                        } else if let Some(from) = source.strip_prefix("from storage ") {
                            let (sid, sp) = split_first_word(from);
                            match self.world.storage.get(sid, &sp.to_owned()) { Some(v) => self.world.entities.nbt_set(eid, path, v), None => Err("src not found".into()) }
                        } else { Err(format!("unknown source: {source}")) },
                        "merge" => if let Some(snbt) = source.strip_prefix("value ") {
                            NbtValue::from_snbt(snbt).and_then(|v| self.world.entities.nbt_merge(eid, path, v))
                        } else { Err("merge needs value".into()) },
                        _ => Err(format!("unknown mode: {mode}")),
                    };
                    return match result { Ok(()) => CommandOutcome::success(1, vec![]), Err(e) => CommandOutcome::failure(vec![e]) };
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

    fn execute_entity_data_get(&self, selector: &str, path: &str, scale: Option<f64>) -> CommandOutcome {
        let ids = self.world.entities.resolve_selector(selector, &self.world.scoreboard);
        let Some(entity_id) = ids.first().copied() else {
            return CommandOutcome::failure(vec!["No entity was found".to_owned()]);
        };
        let Some(value) = self.world.entities.nbt_get(entity_id, path) else {
            return CommandOutcome::failure(vec![format!("Entity {selector} not found at {path}")]);
        };
        let snbt = value.to_snbt();
        let result = match scale { Some(s) => ((value.as_i32() as f64) * s) as i32, None => value.as_i32() };
        CommandOutcome::success(result, vec![format!("{selector} has the following entity data: {snbt}")])
    }

    fn data_modify_set(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        if let Some(snbt) = source.strip_prefix("value ") {
            let val = parse_snbt_literal(snbt)?;
            self.world.storage.set(storage, path, val)
        } else if let Some(from) = source.strip_prefix("from ") {
            let storage_part = from.strip_prefix("storage ").unwrap_or(from);
            let (src_storage, src_path) = split_first_word(storage_part);
            let src_path = src_path.to_owned();
            self.world.storage.copy_from(storage, path, src_storage, &src_path)
        } else {
            Err(format!("unknown data source: {source}"))
        }
    }

    fn data_modify_append(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        if let Some(snbt) = source.strip_prefix("value ") {
            let val = parse_snbt_literal(snbt)?;
            self.world.storage.append(storage, path, val)
        } else if let Some(from) = source.strip_prefix("from ") {
            let storage_part = from.strip_prefix("storage ").unwrap_or(from);
            let (src_storage, src_path) = split_first_word(storage_part);
            let src = self.world.storage.get(src_storage, &src_path)
                .ok_or_else(|| format!("source not found: {src_storage} {src_path}"))?;
            self.world.storage.append(storage, path, src)
        } else {
            Err(format!("expected value or from: {source}"))
        }
    }

    fn data_modify_prepend(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        if let Some(snbt) = source.strip_prefix("value ") {
            let val = parse_snbt_literal(snbt)?;
            self.world.storage.prepend(storage, path, val)
        } else if let Some(from) = source.strip_prefix("from ") {
            let storage_part = from.strip_prefix("storage ").unwrap_or(from);
            let (src_storage, src_path) = split_first_word(storage_part);
            let src = self.world.storage.get(src_storage, &src_path)
                .ok_or_else(|| format!("source not found: {src_storage} {src_path}"))?;
            self.world.storage.prepend(storage, path, src)
        } else {
            Err(format!("expected value or from: {source}"))
        }
    }

    fn data_modify_merge(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        let snbt = source.strip_prefix("value ").ok_or_else(|| format!("expected value: {source}"))?;
        let val = parse_snbt_literal(snbt)?;
        self.world.storage.merge(storage, path, val)
    }

    fn data_modify_from(&mut self, storage: &str, path: &str, source: &str) -> Result<(), String> {
        let from = source.strip_prefix("storage ")
            .or_else(|| source.strip_prefix("from storage "))
            .ok_or_else(|| format!("expected storage: {source}"))?;
        let (src_storage, src_path) = split_first_word(from);
        let src_path = src_path.to_owned();
        self.world.storage.copy_from(storage, path, src_storage, &src_path)
    }

    // ── execute ─────────────────────────────────────────────────

    fn execute_execute(&mut self, cmd: &ExecuteCmd, budget: &CommandBudget) -> CommandOutcome {
        let mut ctx = ExecutionContext::default();
        let mut skip = false;

        for modifier in &cmd.modifiers {
            if skip { break; }
            match modifier {
                ExecuteModifier::As(selector) => {
                    if selector == "@s" {
                        // keep current executor
                    } else {
                        let ids = self.world.entities.resolve_selector(selector, &self.world.scoreboard);
                        if ids.is_empty() { skip = true; continue; }
                        ctx.executor = Some(ids[0]);
                        ctx.entity_ids = ids;
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
                        let ids = self.world.entities.resolve_selector(selector, &self.world.scoreboard);
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
                        "feet" => Anchor::Feet,
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
                    if !self.evaluate_condition(cond, &ctx) { skip = true; }
                }
                ExecuteModifier::Unless(cond) => {
                    if self.evaluate_condition(cond, &ctx) { skip = true; }
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
                ExecuteModifier::StoreSuccess(dest) => { self.store_outcome(dest, i32::from(outcome.success)); }
                ExecuteModifier::StoreResult(dest) => { self.store_outcome(dest, outcome.result); }
                _ => {}
            }
        }
        outcome
    }

    fn execute_nested_with_context(&mut self, cmd: &ParsedCommand, ctx: &ExecutionContext, budget: &CommandBudget) -> CommandOutcome {
        match cmd {
            ParsedCommand::Teleport(tc) => self.execute_teleport_at(ctx, &tc.target, &tc.destination),
            ParsedCommand::Say(msg) => {
                let log = if let Some(id) = ctx.executor {
                    format!("[entity_{id}] {msg}")
                } else {
                    self.execute_say(msg).log.first().cloned().unwrap_or_default()
                };
                CommandOutcome::success(0, vec![log])
            }
            _ => self.execute_parsed(cmd, budget),
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
                    let entry_outcome = self.execute_function_body(&entry, &mut sub_budget);
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

        self.execute_function_body(&name, &mut budget.clone())
    }

    fn execute_function_body(
        &mut self,
        name: &str,
        budget: &mut CommandBudget,
    ) -> CommandOutcome {
        // If the entry is a tag reference (starts with #), dispatch through tag logic.
        if let Some(tag_name) = name.strip_prefix('#') {
            let fc = crate::parse::FunctionCmd { name: tag_name.to_owned(), is_tag: true, with_storage: None, inline_args: None };
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
            let parsed = parse_command(cmd_text);
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

    fn execute_teleport(
        &mut self,
        _cmd: &super::parse::TeleportCmd,
    ) -> CommandOutcome {
        CommandOutcome::success(0, vec![])
    }

    // ── return ──────────────────────────────────────────────────

    fn execute_return(
        &mut self,
        cmd: &ReturnCmd,
        budget: &CommandBudget,
    ) -> CommandOutcome {
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

    fn execute_gamerule(
        &mut self,
        cmd: &super::parse::GameruleCmd,
    ) -> CommandOutcome {
        self.world.gamerules.set(&cmd.rule, cmd.value);
        let log = format!("[Server thread/INFO]: Gamerule {} is now set to: {}", cmd.rule, cmd.value);
        CommandOutcome::success(cmd.value, vec![log])
    }

    // ── schedule ────────────────────────────────────────────────

    fn execute_schedule(
        &mut self,
        cmd: &ScheduleCmd,
    ) -> CommandOutcome {
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

    fn execute_summon(
        &mut self,
        cmd: &super::parse::SummonCmd,
    ) -> CommandOutcome {
        let tags: Vec<String> = cmd.nbt.as_ref()
            .and_then(|nbt| {
                nbt.strip_prefix("{Tags:[")
                    .and_then(|rest| rest.split(']').next())
                    .map(|tags_str| {
                        tags_str.split(',')
                            .map(|t| t.trim().trim_matches('"').to_owned())
                            .collect()
                    })
            })
            .unwrap_or_default();
        let x = cmd.pos.map_or(0.0, |(x, _, _)| x);
        let y = cmd.pos.map_or(0.0, |(_, y, _)| y);
        let z = cmd.pos.map_or(0.0, |(_, _, z)| z);
        let id = self.world.entities.summon(&cmd.entity_type, x, y, z, &tags);
        let type_name = cmd.entity_type
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

fn extract_entity_name(selector: &str) -> String {
    selector.strip_prefix('@').unwrap_or(selector).to_owned()
}

fn evaluate_score_range(value: Option<i32>, range: &str) -> bool {
    let value = value.unwrap_or(0);
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
    if parts.len() >= 1 {
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

fn evaluate_condition_at(exec: &mut Executor, cond: &ExecuteCondition, ctx: &ExecutionContext) -> bool {
    exec.evaluate_condition(cond, ctx)
}

impl Executor {
    fn execute_teleport_at(&mut self, ctx: &ExecutionContext, target: &str, dest: &str) -> CommandOutcome {
        let ids = if target == "@s" {
            ctx.executor.map(|id| vec![id]).unwrap_or_default()
        } else {
            self.world.entities.resolve_selector(target, &self.world.scoreboard)
        };
        if ids.is_empty() {
            return CommandOutcome::failure(vec![]);
        }
        let mut new_x = ctx.x;
        let mut new_y = ctx.y;
        let mut new_z = ctx.z;
        let mut new_yaw = ctx.yaw;
        let mut new_pitch = ctx.pitch;

        let parts: Vec<&str> = dest.split_whitespace().collect();
        if parts.len() >= 3 {
            apply_world_axis(&mut new_x, parts[0]);
            apply_world_axis(&mut new_y, parts[1]);
            apply_world_axis(&mut new_z, parts[2]);
            if parts.len() >= 5 {
                apply_rotation(&mut new_yaw, parts[3]);
                apply_rotation(&mut new_pitch, parts[4]);
            }
        }

        let count = ids.len() as i32;
        for id in &ids {
            self.world.entities.teleport_absolute(*id, new_x, new_y, new_z, new_yaw, new_pitch);
        }
        CommandOutcome::success(count, vec![])
    }

    fn evaluate_condition(&mut self, cond: &ExecuteCondition, ctx: &ExecutionContext) -> bool {
        match cond {
            ExecuteCondition::Score { holder, objective, range } => {
                let value = self.world.scoreboard.get(holder, objective);
                evaluate_score_range(value, range)
            }
            ExecuteCondition::ScoreCompare {
                left_holder, left_objective, op,
                right_holder, right_objective,
            } => {
                let left = self.world.scoreboard.get(left_holder, left_objective).unwrap_or(0);
                let right = self.world.scoreboard.get(right_holder, right_objective).unwrap_or(0);
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
                self.world.storage.exists(storage, path)
            }
            ExecuteCondition::Entity(selector) => {
                self.world.entities.resolve_selector_at(selector, ctx.x, ctx.y, ctx.z, &self.world.scoreboard).len() > 0
            }
            ExecuteCondition::Function(name) => {
                // Vanilla: execute if function RUNS the function and succeeds
                // when the function completed successfully (success=1, no return fail).
                let fc = crate::parse::FunctionCmd { name: name.clone(), is_tag: false, with_storage: None, inline_args: None };
                let outcome = self.execute_function_call(&fc, &CommandBudget {
                    sequence_remaining: u32::MAX,
                    fork_limit: u32::MAX,
                });
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
            let _ = self.world.storage.store_numeric(&storage, &path, numeric_type, scale, value);
        } else if words.len() >= 3 && words[0] == "entity" {
            // execute store result entity @s Pos[0] double 0.001
            let selector = &words[1];
            let path = &words[2];
            let numeric_type = words.get(3).map_or("double", |s| *s);
            let scale: f64 = words.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let scaled = (value as f64) * scale;
            let ids = self.world.entities.resolve_selector(selector, &self.world.scoreboard);
            for id in ids {
                let Some(e) = self.world.entities.get(id) else { continue; };
                let (nx, ny, nz) = match &**path {
                    "Pos[0]" => (scaled, e.y, e.z),
                    "Pos[1]" => (e.x, scaled, e.z),
                    "Pos[2]" => (e.x, e.y, scaled),
                    _ => (e.x, e.y, e.z),
                };
                self.world.entities.teleport_absolute(id, nx, ny, nz, e.yaw, e.pitch);
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

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
    }
}

/// Parses a simple SNBT literal like `3`, `-5b`, `"hello"`, `[1,2,3]`, `{a:1,b:2}`.
fn parse_snbt_literal(snbt: &str) -> Result<NbtValue, String> {
    let s = snbt.trim();
    if s.starts_with('{') {
        parse_snbt_compound(s)
    } else if s.starts_with('[') {
        parse_snbt_list(s)
    } else if s.starts_with('"') {
        parse_snbt_string(s)
    } else if let Some(rest) = s.strip_suffix('b') {
        Ok(NbtValue::Byte(rest.parse().map_err(|_| format!("invalid byte: {rest}"))?))
    } else if let Some(rest) = s.strip_suffix('s') {
        Ok(NbtValue::Short(rest.parse().map_err(|_| format!("invalid short: {rest}"))?))
    } else if let Some(rest) = s.strip_suffix('L') {
        Ok(NbtValue::Long(rest.parse().map_err(|_| format!("invalid long: {rest}"))?))
    } else if let Some(rest) = s.strip_suffix('f') {
        Ok(NbtValue::Float(rest.parse().map_err(|_| format!("invalid float: {rest}"))?))
    } else if let Some(rest) = s.strip_suffix('d') {
        Ok(NbtValue::Double(rest.parse().map_err(|_| format!("invalid double: {rest}"))?))
    } else {
        Ok(NbtValue::Int(s.parse().map_err(|_| format!("invalid int: {s}"))?))
    }
}

fn parse_snbt_string(s: &str) -> Result<NbtValue, String> {
    let s = s.trim();
    let unquoted = s.strip_prefix('"').and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| format!("invalid string: {s}"))?;
    let mut result = String::new();
    let mut chars = unquoted.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some(c) => result.push(c),
                None => break,
            }
        } else {
            result.push(c);
        }
    }
    Ok(NbtValue::String(result))
}

fn parse_snbt_list(s: &str) -> Result<NbtValue, String> {
    let s = s.trim();
    let inner = s.strip_prefix('[').and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| format!("invalid list: {s}"))?;
    if inner.is_empty() {
        return Ok(NbtValue::List(vec![]));
    }
    let elements = split_snbt_elements(inner);
    let values: Result<Vec<_>, _> = elements.iter().map(|e| parse_snbt_literal(e)).collect();
    Ok(NbtValue::List(values?))
}

fn parse_snbt_compound(s: &str) -> Result<NbtValue, String> {
    let s = s.trim();
    let inner = s.strip_prefix('{').and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| format!("invalid compound: {s}"))?;
    if inner.is_empty() {
        return Ok(NbtValue::compound(vec![]));
    }
    let pairs = split_snbt_elements(inner);
    let mut entries = Vec::new();
    for pair in pairs {
        let colon = pair.find(':').ok_or_else(|| format!("missing colon in: {pair}"))?;
        let key = pair[..colon].trim();
        let val = pair[colon + 1..].trim();
        let key = key.strip_prefix('"').and_then(|k| k.strip_suffix('"')).unwrap_or(key);
        entries.push((key.to_owned(), parse_snbt_literal(val)?));
    }
    Ok(NbtValue::compound(entries))
}

fn split_snbt_elements(s: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    let mut in_string = false;

    for c in s.chars() {
        match c {
            '"' => {
                in_string = !in_string;
                current.push(c);
            }
            '{' | '[' if !in_string => {
                depth += 1;
                current.push(c);
            }
            '}' | ']' if !in_string => {
                depth -= 1;
                current.push(c);
            }
            ',' if !in_string && depth == 0 => {
                elements.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        elements.push(current);
    }
    elements
}
