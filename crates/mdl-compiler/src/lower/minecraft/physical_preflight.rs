//! Target validation for selected physical materialization and activation recipes.
//!
//! Semantic target preflight runs before physical locations exist. This separate
//! immutable product validates the concrete score/frame recipe inventory after
//! realization planning and before target resources are allocated.

use std::error::Error;
use std::fmt;

use crate::target::JavaEditionTarget;

use super::realization::PhysicalRealizationPlan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalPreflight {
    target: JavaEditionTarget,
    constant_bool_to_score: usize,
    constant_i32_to_score: usize,
    recursive_edges: usize,
    score_bool_to_frame: usize,
    score_i32_to_frame: usize,
    frame_bool_to_score: usize,
    frame_i32_to_score: usize,
    frame_resets: usize,
    local_sequence_operations: usize,
    local_forks: usize,
}

impl PhysicalPreflight {
    pub(crate) fn new(
        target: JavaEditionTarget,
        physical: &PhysicalRealizationPlan,
    ) -> Result<Self, PhysicalPreflightError> {
        let recursive_edges = physical.recursive_call_count();
        let inventory = physical.recipe_inventory();
        let score_to_frame = inventory.score_bool_to_frame + inventory.score_i32_to_frame;
        let constants = inventory.constant_bool_to_score + inventory.constant_i32_to_score;
        let frame_resets = usize::from(physical.has_recursive_activation());
        let candidate = Self {
            target,
            constant_bool_to_score: inventory.constant_bool_to_score,
            constant_i32_to_score: inventory.constant_i32_to_score,
            recursive_edges,
            score_bool_to_frame: inventory.score_bool_to_frame,
            score_i32_to_frame: inventory.score_i32_to_frame,
            frame_bool_to_score: inventory.score_bool_to_frame,
            frame_i32_to_score: inventory.score_i32_to_frame,
            frame_resets,
            local_sequence_operations: constants
                + score_to_frame.saturating_mul(2)
                + recursive_edges.saturating_mul(2)
                + frame_resets,
            local_forks: 0,
        };
        candidate.verify(physical)?;
        Ok(candidate)
    }

    pub(crate) fn verify(
        self,
        physical: &PhysicalRealizationPlan,
    ) -> Result<(), PhysicalPreflightError> {
        let inventory = physical.recipe_inventory();
        let constants = self.constant_bool_to_score + self.constant_i32_to_score;
        let score_to_frame = self.score_bool_to_frame + self.score_i32_to_frame;
        if self.recursive_edges != physical.recursive_call_count()
            || self.constant_bool_to_score != inventory.constant_bool_to_score
            || self.constant_i32_to_score != inventory.constant_i32_to_score
            || constants != physical.materialization_count()
            || self.score_bool_to_frame != inventory.score_bool_to_frame
            || self.score_i32_to_frame != inventory.score_i32_to_frame
            || score_to_frame != physical.recursive_spill_count()
            || self.frame_bool_to_score != self.score_bool_to_frame
            || self.frame_i32_to_score != self.score_i32_to_frame
            || self.frame_resets != usize::from(physical.has_recursive_activation())
            || self.local_sequence_operations
                != constants
                    + score_to_frame.saturating_mul(2)
                    + self.recursive_edges.saturating_mul(2)
                    + self.frame_resets
            || self.local_forks != 0
        {
            return Err(PhysicalPreflightError::InventoryMismatch);
        }
        // Java 26.2 is currently the only supported target. Keeping the match here
        // makes target admission an explicit physical-recipe decision when another
        // target is introduced.
        match self.target {
            JavaEditionTarget::V26_2 => Ok(()),
        }
    }

    pub(crate) const fn recursive_edges(self) -> usize {
        self.recursive_edges
    }

    pub(crate) const fn spill_bridges(self) -> usize {
        self.score_bool_to_frame + self.score_i32_to_frame
    }

    pub(crate) const fn local_sequence_operations(self) -> usize {
        self.local_sequence_operations
    }

    pub(crate) const fn local_forks(self) -> usize {
        self.local_forks
    }

    #[cfg(test)]
    pub(crate) fn corrupt_spill_bridges(&mut self) {
        self.score_i32_to_frame = self.score_i32_to_frame.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalPreflightError {
    InventoryMismatch,
}

impl fmt::Display for PhysicalPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InventoryMismatch => {
                formatter.write_str("physical recipe inventory disagrees with realization plan")
            }
        }
    }
}

impl Error for PhysicalPreflightError {}
