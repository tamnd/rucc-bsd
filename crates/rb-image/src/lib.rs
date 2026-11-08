//! Disk images for rucc-bsd (plan 12.3 and 12.4).
//!
//! Every image rb boots carries rb-rc, the smoke suite and an `rb.conf` ([`guest`], [`conf`]).
//! rb-rc reports through a second disk that holds a tar archive with no file system ([`ustar`]).
//! [`image`] makes the image from a build: [`freebsd`] with `makefs` and `mkimg`, [`netbsd`] with
//! the project's `live-image` target. [`plan`] holds the commands and runs them.

pub mod conf;
pub mod freebsd;
pub mod guest;
pub mod image;
pub mod netbsd;
pub mod plan;
pub mod ustar;
