//! Compiler implementation for MDL.
//!
//! The compiler is being built in independently verifiable layers. The current
//! layers provide typed entity identity, source provenance, Core SSA, a verified
//! structured Minecraft IR, checked Core-to-Minecraft lowering, and deterministic
//! datapack emission. There is intentionally no source parser yet.

pub mod analysis;
pub mod datapack;
pub mod diagnostic;
mod entity;
pub mod ir;
pub mod lower;
pub mod opt;
pub mod source;
pub mod target;
