//! The library behind `rb`: fetch pinned FreeBSD and NetBSD trees, build them with rucc and with
//! a reference GCC, and record what happened.
//!
//! The commands arrive one milestone at a time, in the order of plan 16. Each one lives in its
//! own module, and the binary only parses the command line and calls it.

pub mod boot;
pub mod census;
pub mod cli;
pub mod console;
pub mod demands;
pub mod fetch;
pub mod image;
pub mod personas;
pub mod repo;
pub mod scan;
pub mod sets;
