//! Disk images for rucc-bsd (plan 12.3 and 12.4).
//!
//! Every image rb boots carries rb-rc, the smoke suite and an `rb.conf` ([`guest`], [`conf`]).
//! rb-rc reports through a second disk that holds a tar archive with no file system ([`ustar`]).

pub mod conf;
pub mod guest;
pub mod ustar;
