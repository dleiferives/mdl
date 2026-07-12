//! Compiler implementation for MDL.
//!
//! The compiler is being built in independently verifiable layers. The current
//! layers provide typed entity identity, source provenance, Core SSA, a verified
//! structured Minecraft IR, and deterministic datapack emission. There is
//! intentionally no parser or Core-to-Minecraft lowering yet.

pub mod datapack;
pub mod diagnostic;
mod entity;
pub mod ir;
pub mod source;
pub mod target;
