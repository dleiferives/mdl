mod parser;

pub use parser::parse_command;

/// Outcome of executing a single command.
#[derive(Clone, Debug, Default)]
pub struct CommandOutcome {
    pub success: u8,
    pub result: i32,
    pub continued: bool,
    pub log: Vec<String>,
}

impl CommandOutcome {
    #[must_use]
    pub fn success(result: i32, log: Vec<String>) -> Self {
        Self {
            success: 1,
            result,
            continued: true,
            log,
        }
    }

    #[must_use]
    pub fn failure(log: Vec<String>) -> Self {
        Self {
            success: 0,
            result: 0,
            continued: true,
            log,
        }
    }

    #[must_use]
    pub fn returned(value: i32, log: Vec<String>) -> Self {
        Self {
            success: 1,
            result: value,
            continued: false,
            log,
        }
    }

    #[must_use]
    pub fn returned_fail(log: Vec<String>) -> Self {
        Self {
            success: 0,
            result: 0,
            continued: false,
            log,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ParsedCommand {
    Scoreboard(ScoreboardCmd),
    Data(DataCmd),
    Execute(ExecuteCmd),
    Function(FunctionCmd),
    Say(String),
    Teleport(TeleportCmd),
    Return(ReturnCmd),
    Gamerule(GameruleCmd),
    Schedule(ScheduleCmd),
    Summon(SummonCmd),
    Forceload(ForceloadCmd),
    Setblock(SetblockCmd),
    Kill(KillCmd),
    Tag(TagCmd),
    Tellraw(TellrawCmd),
    Title(TitleCmd),
    Playsound,
    Loot(LootCmd),
    Rotate(RotateCmd),
    Item(ItemCmd),
    Advancement(AdvancementCmd),
    Reload,
    Stop,
    Raw(String),
}

#[derive(Clone, Debug)]
pub struct ScoreboardCmd {
    pub subcommand: ScoreboardSub,
}

#[derive(Clone, Debug)]
pub enum ScoreboardSub {
    ObjectivesAdd {
        objective: String,
        criterion: String,
    },
    PlayersSet {
        holder: String,
        objective: String,
        value: i32,
    },
    PlayersAdd {
        holder: String,
        objective: String,
        amount: i32,
    },
    PlayersRemove {
        holder: String,
        objective: String,
        amount: i32,
    },
    PlayersGet {
        holder: String,
        objective: String,
    },
    PlayersReset {
        holder: String,
        objective: String,
    },
    PlayersOperation {
        target_holder: String,
        target_objective: String,
        op: String,
        source_holder: String,
        source_objective: String,
    },
}

#[derive(Clone, Debug)]
pub struct DataCmd {
    pub subcommand: DataSub,
}

#[derive(Clone, Debug)]
pub enum DataSub {
    Get {
        storage: String,
        path: String,
        scale: Option<f64>,
    },
    Remove {
        storage: String,
        path: String,
    },
    Modify {
        storage: String,
        path: String,
        mode: String,
        source: String,
    },
}

#[derive(Clone, Debug)]
pub struct ExecuteCmd {
    pub modifiers: Vec<ExecuteModifier>,
    pub nested: Box<ParsedCommand>,
}

#[derive(Clone, Debug)]
pub enum ExecuteModifier {
    As(String),
    At(String),
    In(String),
    Positioned(String),
    Rotated(String),
    Anchored(String),
    Align(String),
    If(ExecuteCondition),
    Unless(ExecuteCondition),
    StoreSuccess(String),
    StoreResult(String),
}

#[derive(Clone, Debug)]
pub enum ExecuteCondition {
    Score {
        holder: String,
        objective: String,
        range: String,
    },
    ScoreCompare {
        left_holder: String,
        left_objective: String,
        op: String,
        right_holder: String,
        right_objective: String,
    },
    Data(String, String),
    Entity(String),
    Function(String),
    Block {
        x: i32,
        y: i32,
        z: i32,
        block: String,
    },
}

#[derive(Clone, Debug)]
pub struct FunctionCmd {
    pub name: String,
    pub is_tag: bool,
    pub with_storage: Option<(String, String)>,
    pub inline_args: Option<String>,
}

#[derive(Clone, Debug)]
pub struct KillCmd {
    pub selector: String,
}

#[derive(Clone, Debug)]
pub struct TagCmd {
    pub selector: String,
    pub action: TagAction,
    pub tag: String,
}

#[derive(Clone, Debug)]
pub enum TagAction {
    Add,
    Remove,
    List,
}

#[derive(Clone, Debug)]
pub struct TellrawCmd {
    pub selector: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct TitleCmd {
    pub selector: String,
    pub action: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct LootCmd {
    pub action: String,
    pub pos: Option<(f64, f64, f64)>,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct RotateCmd {
    pub selector: String,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Debug)]
pub struct ItemCmd {
    pub action: String,
    pub selector: String,
    pub slot: String,
    pub rest: String,
}

#[derive(Clone, Debug)]
pub struct AdvancementCmd {
    pub action: String,
    pub selector: String,
    pub advancement: String,
}

#[derive(Clone, Debug)]
pub struct TeleportCmd {
    pub target: String,
    pub destination: String,
}

#[derive(Clone, Debug)]
pub enum ReturnCmd {
    Value(i32),
    Fail,
    Run(Box<ParsedCommand>),
}

#[derive(Clone, Debug)]
pub struct GameruleCmd {
    pub rule: String,
    pub value: i32,
}

#[derive(Clone, Debug)]
pub struct ScheduleCmd {
    pub function: String,
    pub delay_ticks: u32,
    pub replace: bool,
}

#[derive(Clone, Debug)]
pub struct SummonCmd {
    pub entity_type: String,
    pub pos: Option<(f64, f64, f64)>,
    pub nbt: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ForceloadCmd {
    pub add: bool,
    pub x: i32,
    pub z: i32,
}

#[derive(Clone, Debug)]
pub struct SetblockCmd {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: String,
}
