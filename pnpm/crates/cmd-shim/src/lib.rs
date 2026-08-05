//! Bin field parsing and command-shim generation. Three concerns:
//!
//! - Parsing the `bin` (and `directories.bin`) field of a package's
//!   `package.json` into a list of `(name, path)` commands.
//! - Orchestrating the per-`node_modules` linking pass and the conflict
//!   resolution between bins of the same name.
//! - Generating the actual shim file contents.
//!
//! Pacquet's first iteration covers the direct-dependency path (root project's
//! `node_modules/.bin`) and the per-virtual-store path
//! (`node_modules/.pacquet/<pkg>@<ver>/node_modules/<pkg>/node_modules/.bin`).
//! Hoisted-bin precedence and lifecycle-script-created bins are deferred per
//! `plans/TEST_PORTING.md`.

mod bin_resolver;
mod capabilities;
mod link_bins;
mod path_util;
mod shim;

pub use bin_resolver::*;
pub use capabilities::*;
pub use link_bins::*;
pub use shim::*;
