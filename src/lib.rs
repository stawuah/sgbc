//! Solana Ghana Builder Cloud — deployment library.
//!
//! The binary is a thin shell over these modules so the generated systemd and
//! Caddy output can be tested directly, without a host to deploy to.

pub mod config;
pub mod layout;
pub mod plan;
pub mod render;
