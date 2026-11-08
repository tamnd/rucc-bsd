//! The sets and pins of rucc-bsd (plan 04.5 to 04.7).
//!
//! `sets.toml` states rules: which branches, and which releases the projects support.
//! [`expand::expand`] reads what FreeBSD and NetBSD publish today and turns the rules into the
//! pins of `pins.toml`, each a commit or a tarball with the digest the project publishes.

pub mod expand;
pub mod pins;
pub mod places;
pub mod publish;
pub mod sets;
pub mod toolchains;
pub mod web;
