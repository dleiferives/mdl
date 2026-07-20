use std::error::Error;
use std::fmt;

use crate::ir::command_line::{CommandLineShapeError, validate_command_line_shape};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

use super::macro_command::MacroCommand;
use super::{
    CallableRef, DataCommand, ExecuteCommand, SayCommand, ScoreCommand, StoragePath,
    TeleportCommand,
};

/// Initial maximum nesting depth for recursive commands.
pub const MAX_COMMAND_DEPTH: usize = 64;

/// One command with line-level provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandNode {
    pub(super) kind: CommandKind,
    origin: OriginId,
}

impl CommandNode {
    #[cfg(test)]
    pub(super) const fn from_unchecked(kind: CommandKind, origin: OriginId) -> Self {
        Self { kind, origin }
    }

    /// Constructs a command and enforces the structural-depth limit.
    ///
    /// # Errors
    ///
    /// Rejects a recursive command deeper than [`MAX_COMMAND_DEPTH`].
    pub fn new(kind: CommandKind, origin: OriginId) -> Result<Self, CommandDepthError> {
        let node = Self { kind, origin };
        node.validate_depth()?;
        Ok(node)
    }

    /// Returns the command semantics.
    #[must_use]
    pub const fn kind(&self) -> &CommandKind {
        &self.kind
    }

    /// Returns the command provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns recursive structural depth, where a non-recursive command has depth
    /// one.
    #[must_use]
    pub fn depth(&self) -> usize {
        1 + match &self.kind {
            CommandKind::Execute(command) => command.run().depth(),
            CommandKind::Return(ReturnCommand::Run(command)) => command.depth(),
            CommandKind::Score(_)
            | CommandKind::Data(_)
            | CommandKind::Say(_)
            | CommandKind::Teleport(_)
            | CommandKind::Function(_)
            | CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail)
            | CommandKind::Raw(_)
            | CommandKind::Macro(_)
            | CommandKind::FunctionWithStorage(_) => 0,
        }
    }

    fn validate_depth(&self) -> Result<(), CommandDepthError> {
        let depth = self.depth();
        if depth > MAX_COMMAND_DEPTH {
            Err(CommandDepthError {
                depth,
                max: MAX_COMMAND_DEPTH,
            })
        } else {
            Ok(())
        }
    }
}

/// The closed initial command vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandKind {
    /// Scoreboard command.
    Score(ScoreCommand),
    /// Storage-data command.
    Data(DataCommand),
    /// Literal native `say` command.
    Say(SayCommand),
    /// Teleport the current executor to a structured position.
    Teleport(TeleportCommand),
    /// Ordered execute chain.
    Execute(ExecuteCommand),
    /// Function or function-tag invocation.
    Function(FunctionCall),
    /// Return from the current function invocation.
    Return(ReturnCommand),
    /// Explicit unparsed escape hatch.
    Raw(UnsafeRawCommand),
    /// A typed macro function command.
    Macro(MacroCommand),
    /// A function invocation with storage macro arguments.
    FunctionWithStorage(FunctionWithStorage),
}

/// One typed function or function-tag invocation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FunctionCall {
    target: CallableRef,
}

/// A function invocation carrying a storage compound for macro argument
/// substitution via `function <target> with storage <path>`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FunctionWithStorage {
    pub target: CallableRef,
    pub storage: StoragePath,
}

impl FunctionWithStorage {
    /// Constructs a function-with-storage call.
    #[must_use]
    pub const fn new(target: CallableRef, storage: StoragePath) -> Self {
        Self { target, storage }
    }
}

impl FunctionCall {
    /// Constructs a call to an internal or external target.
    #[must_use]
    pub const fn new(target: CallableRef) -> Self {
        Self { target }
    }

    /// Returns the callable target.
    #[must_use]
    pub const fn target(&self) -> &CallableRef {
        &self.target
    }
}

/// One native `return` shape.
#[derive(Clone, Debug, PartialEq)]
pub enum ReturnCommand {
    /// Return an explicit integer result successfully.
    Value(i32),
    /// Return failure.
    Fail,
    /// Run a command and propagate its result and success.
    Run(Box<CommandNode>),
}

impl ReturnCommand {
    /// Wraps one already validated nested command.
    #[must_use]
    pub fn run(command: CommandNode) -> Self {
        Self::Run(Box::new(command))
    }
}

/// A command nesting deeper than the compiler safety limit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandDepthError {
    depth: usize,
    max: usize,
}

impl CommandDepthError {
    /// Returns the rejected depth.
    #[must_use]
    pub const fn depth(self) -> usize {
        self.depth
    }

    /// Returns the compiler maximum.
    #[must_use]
    pub const fn max(self) -> usize {
        self.max
    }
}

impl fmt::Display for CommandDepthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "command depth {} exceeds compiler limit {}",
            self.depth, self.max
        )
    }
}

impl Error for CommandDepthError {}

/// Why an unsafe raw command line was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RawCommandError {
    /// No command text was supplied.
    Empty,
    /// The line contains a physical CR or LF.
    PhysicalNewline,
    /// The line begins or ends with whitespace.
    BoundaryWhitespace,
    /// The first character is reserved for slash input, comments, or macros.
    ReservedPrefix(char),
    /// A terminal backslash would continue the next physical line.
    TerminalContinuation,
    /// The line exceeds the target's Java UTF-16 bound.
    TooLong {
        /// Observed Java UTF-16 code-unit count.
        units: usize,
        /// Target maximum.
        max: usize,
    },
}

impl fmt::Display for RawCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("raw command cannot be empty"),
            Self::PhysicalNewline => formatter.write_str("raw command cannot contain CR or LF"),
            Self::BoundaryWhitespace => {
                formatter.write_str("raw command cannot begin or end with whitespace")
            }
            Self::ReservedPrefix(prefix) => {
                write!(formatter, "raw command cannot start with {prefix:?}")
            }
            Self::TerminalContinuation => {
                formatter.write_str("raw command cannot end with a continuation backslash")
            }
            Self::TooLong { units, max } => {
                write!(
                    formatter,
                    "raw command has {units} UTF-16 units; maximum is {max}"
                )
            }
        }
    }
}

impl Error for RawCommandError {}

/// One explicitly unsafe, unparsed physical command line.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnsafeRawCommand(Box<str>);

impl UnsafeRawCommand {
    #[cfg(test)]
    pub(super) fn from_unchecked(line: &str) -> Self {
        Self(line.into())
    }

    /// Validates one physical raw command against the only current target.
    ///
    /// This does not parse Brigadier syntax or establish semantic contracts.
    ///
    /// # Errors
    ///
    /// Rejects physical-line hazards, reserved prefixes, boundary whitespace, and
    /// target-length overflow.
    pub fn new(line: &str) -> Result<Self, RawCommandError> {
        Self::new_for_target(line, JavaEditionTarget::V26_2)
    }

    /// Validates one physical raw command against an explicit Java target.
    ///
    /// This does not parse Brigadier syntax or establish semantic contracts.
    ///
    /// # Errors
    ///
    /// Rejects target-independent physical-line hazards and target-length overflow.
    pub fn new_for_target(line: &str, target: JavaEditionTarget) -> Result<Self, RawCommandError> {
        validate_raw_line(line, target)?;
        Ok(Self(line.into()))
    }

    /// Returns the unparsed physical command text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_raw_line(line: &str, target: JavaEditionTarget) -> Result<(), RawCommandError> {
    validate_command_line_shape(line).map_err(|error| match error {
        CommandLineShapeError::Empty => RawCommandError::Empty,
        CommandLineShapeError::PhysicalNewline => RawCommandError::PhysicalNewline,
        CommandLineShapeError::BoundaryWhitespace => RawCommandError::BoundaryWhitespace,
        CommandLineShapeError::ReservedPrefix(prefix) => RawCommandError::ReservedPrefix(prefix),
        CommandLineShapeError::TerminalContinuation => RawCommandError::TerminalContinuation,
    })?;
    let units = line.encode_utf16().count();
    let max = usize::try_from(target.spec().max_logical_command_utf16_units())
        .expect("target command length fits usize");
    if units > max {
        return Err(RawCommandError::TooLong { units, max });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CommandKind, CommandNode, FunctionCall, MAX_COMMAND_DEPTH, RawCommandError,
        UnsafeRawCommand,
    };
    use crate::ir::minecraft::{
        AtMostOneSelector, ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers,
        ExternalCallableRef, FunctionResourceId,
    };
    use crate::source::OriginId;

    fn leaf() -> CommandNode {
        CommandNode::new(
            CommandKind::Function(FunctionCall::new(
                ExternalCallableRef::Function(FunctionResourceId::parse("mdl:leaf").unwrap())
                    .into(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    fn wrap(command: CommandNode) -> Result<CommandNode, super::CommandDepthError> {
        let modifier = ExecuteModifier::new(
            ExecuteModifierKind::As(AtMostOneSelector::SelfExecutor.into()),
            OriginId::UNKNOWN,
        );
        CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(modifier, vec![]),
                command,
            )),
            OriginId::UNKNOWN,
        )
    }

    #[test]
    fn recursive_command_depth_accepts_64_and_rejects_65() {
        let mut command = leaf();
        for _ in 1..MAX_COMMAND_DEPTH {
            command = wrap(command).unwrap();
        }
        assert_eq!(command.depth(), MAX_COMMAND_DEPTH);

        let error = wrap(command).unwrap_err();
        assert_eq!(error.depth(), MAX_COMMAND_DEPTH + 1);
        assert_eq!(error.max(), MAX_COMMAND_DEPTH);
    }

    #[test]
    fn raw_lines_reject_every_physical_hazard() {
        let rejected = [
            ("", RawCommandError::Empty),
            ("say a\nb", RawCommandError::PhysicalNewline),
            ("say a\rb", RawCommandError::PhysicalNewline),
            (" say hi", RawCommandError::BoundaryWhitespace),
            ("say hi ", RawCommandError::BoundaryWhitespace),
            ("/say hi", RawCommandError::ReservedPrefix('/')),
            ("# comment", RawCommandError::ReservedPrefix('#')),
            ("$say $(x)", RawCommandError::ReservedPrefix('$')),
            ("say hi\\", RawCommandError::TerminalContinuation),
        ];
        for (line, expected) in rejected {
            assert_eq!(
                UnsafeRawCommand::new(line).unwrap_err(),
                expected,
                "{line:?}"
            );
        }
        assert_eq!(UnsafeRawCommand::new("say hi").unwrap().as_str(), "say hi");
    }

    #[test]
    fn raw_length_counts_java_utf16_units() {
        let max = usize::try_from(
            crate::target::JavaEditionTarget::V26_2
                .spec()
                .max_logical_command_utf16_units(),
        )
        .unwrap();
        let exact = "a".repeat(max);
        assert!(UnsafeRawCommand::new(&exact).is_ok());

        let over = format!("{exact}a");
        assert!(matches!(
            UnsafeRawCommand::new(&over),
            Err(RawCommandError::TooLong { units, max: found })
                if units == max + 1 && found == max
        ));
        assert!(UnsafeRawCommand::new("say 😀").is_ok());
    }
}
