//! Bin field parsing and command-shim generation.
//!
//! Mirrors three pnpm v11 packages:
//!
//! - `@pnpm/bins.resolver` parses the `bin` (and `directories.bin`) field of a
//!   package's `package.json` into a list of `(name, path)` commands. See
//!   <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/src/index.ts>.
//! - `@pnpm/bins.linker` orchestrates the per-`node_modules` linking pass and
//!   the conflict resolution between bins of the same name. See
//!   <https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts>.
//! - `@zkochan/cmd-shim` generates the actual shim file contents. See
//!   <https://github.com/pnpm/cmd-shim/blob/0d79ca9534/src/index.ts>.
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
