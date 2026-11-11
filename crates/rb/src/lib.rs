//! The library behind `rb`: fetch pinned FreeBSD and NetBSD trees, build them with rucc and with
//! a reference GCC, and record what happened.
//!
//! The commands arrive one milestone at a time, in the order of plan 16. Each one lives in its
//! own module, and the binary only parses the command line and calls it.

pub mod attribution;
pub mod baseline;
pub mod boot;
pub mod build;
pub mod cases;
pub mod census;
pub mod cli;
pub mod console;
pub mod ctf;
pub mod ctfdiff;
pub mod demands;
pub mod divergences;
pub mod fetch;
pub mod flagsdiff;
pub mod frames;
pub mod image;
pub mod mkdiff;
pub mod objdiff;
pub mod personas;
pub mod probes;
pub mod repo;
pub mod scan;
pub mod sets;
pub mod shims;
pub mod syntax;
pub mod toolchain;
pub mod vecaudit;
pub mod warnings;
