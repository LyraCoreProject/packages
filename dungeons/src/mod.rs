//! The `dungeons` Package: encounter choreography, one submodule per dungeon. The registry scan
//! resolves every file through `crate::pkg_dungeons`, so a submodule's markers must stay visible
//! through the glob re-exports below. Adding a dungeon is one new submodule file plus its
//! `mod`/`pub use` pair. See README.md for the Package boundary.

mod deadmines;
mod deadmines_verify;

pub(crate) use deadmines::*;
