//! Bin field parsing and command-shim generation. Concerns:
//!
//! - Parsing the `bin` (and `directories.bin`) field of a package's
//!   `package.json` into a list of `(name, path)` commands.
//! - Orchestrating the per-`node_modules` linking pass and the conflict
//!   resolution between bins of the same name.
//! - Generating the actual shim file contents.

pub use bin_resolver::*;
pub use capabilities::*;
pub use link_bins::*;
pub use shim::*;

mod bin_resolver;
mod capabilities;
mod link_bins;
mod path_util;
mod shim;
