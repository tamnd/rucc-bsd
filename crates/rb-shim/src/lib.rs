//! The compiler shims of `rucc-bsd`: `rb-cc`, `rb-c++`, `rb-cpp` and `rb-as` (plan 14.4), and the
//! `ld` slot of FreeBSD ([`link`]).
//!
//! One binary with many names. A BSD build calls it through its toolchain description, which is
//! FreeBSD's `CROSS_TOOLCHAIN` file or NetBSD's `TOOLDIR`, and never through an edit to the tree.
//! The binary reads the name it was called by, decides which real compiler runs ([`route`]), runs
//! it, and appends one record to `compile.jsonl` through `harness-shim`. The record carries the
//! slot, the side and the origin that `rb census` counts (10.4).
//!
//! The shim changes no argument the build wrote. In R it adds rucc's target and persona in front,
//! and a sysroot where the reference has one built in. Under `RB_BRINGUP` it can hand a call to
//! the reference, which marks the call delegated and the run ungraded.

pub mod config;
pub mod link;
pub mod route;
