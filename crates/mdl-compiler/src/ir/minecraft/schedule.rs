use crate::ir::core::ScheduleMode;

use super::McFunctionId;

/// One typed `schedule function <target> <delay>t [append|replace]` command
/// (Stage 9B). `target`/`delay_ticks`/`mode` are always compile-time literals
/// — there is nothing to marshal, so this mirrors
/// [`super::AdvancementRevokeCommand`]'s minimal shape rather than anything
/// crossings.rs-involved.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScheduleCommand {
    target: McFunctionId,
    delay_ticks: u32,
    mode: ScheduleMode,
}

impl ScheduleCommand {
    /// Constructs a schedule-arm command for `target`.
    #[must_use]
    pub const fn new(target: McFunctionId, delay_ticks: u32, mode: ScheduleMode) -> Self {
        Self {
            target,
            delay_ticks,
            mode,
        }
    }

    /// Returns the function this command schedules.
    #[must_use]
    pub const fn target(&self) -> McFunctionId {
        self.target
    }

    /// Returns the delay in ticks.
    #[must_use]
    pub const fn delay_ticks(&self) -> u32 {
        self.delay_ticks
    }

    /// Returns the schedule pending-slot mode.
    #[must_use]
    pub const fn mode(&self) -> ScheduleMode {
        self.mode
    }
}

/// One typed `schedule clear <target>` command (Stage 9B).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScheduleClearCommand {
    target: McFunctionId,
}

impl ScheduleClearCommand {
    /// Constructs a schedule-clear command for `target`.
    #[must_use]
    pub const fn new(target: McFunctionId) -> Self {
        Self { target }
    }

    /// Returns the function whose pending schedule entry this command clears.
    #[must_use]
    pub const fn target(&self) -> McFunctionId {
        self.target
    }
}
