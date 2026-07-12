//! Compiler implementation for MDL.
//!
//! The compiler is being built in independently verifiable layers. The current
//! layer provides typed entity identity, source provenance, and the initial Core
//! IR types; it intentionally has no parser, control-flow graph, or Minecraft
//! lowering yet.

mod entity;
pub mod ir;
pub mod source;
