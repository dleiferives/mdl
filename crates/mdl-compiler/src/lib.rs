//! Compiler implementation for MDL.
//!
//! The compiler is being built in independently verifiable layers. The current
//! layers provide typed entity identity, source provenance, a bounded typed source
//! frontend, Core SSA, a verified structured Minecraft IR, checked optimization and
//! lowering, target-cost analysis, and deterministic datapack emission.

pub mod analysis;
pub mod datapack;
pub mod diagnostic;
mod entity;
pub mod frontend;
pub mod ir;
pub mod lower;
pub mod opt;
pub mod source;
pub mod target;
