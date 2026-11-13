# crate-layers

Checks the Rust workspace's crate dependency graph against the layers in
[`crate-layers.toml`](../../../crate-layers.toml).

- `just crate-layers check` fails when a crate's normal or build dependency is
  in a higher layer, when a crate is in no layer, or when an exception no
  longer matches an upward edge. It also fails when a
  [ratchet](#ratchets) value differs from its entry, and when a crate
  exceeds its layer's [transitive budget](#transitive-budgets). The
  [product boundary](#product-boundary) list is checked too. CI runs it in
  the `Crate Layers` job.
- `just crate-layers modules` lists the crates whose top-level modules form
  a cycle. `--crate <name>` prints one crate's cycle and the references inside
  it. `--baseline` prints a `module_cycles` table for the current state.
- `just crate-layers dependencies` lists how many workspace crates each crate
  depends on, directly and transitively. `--crate <name>` lists one crate's
  dependencies. `--baseline` prints a `direct_dependencies` table for the
  current state.
- `just crate-layers leaks` lists the [leaks](#leaks): for each crate, the
  workspace dependencies whose items it names in its public signatures
  without re-exporting them. `--crate <name>` lists one crate's leaked items and the
  crates that depend on both. `--baseline` prints a `leaked_dependencies`
  table for the current state.
- `just crate-layers diff <rev>` prints, as Markdown, the crates and edges the
  working tree adds or removes compared to `<rev>`. CI writes it to the job
  summary of every pull request.
- `just crate-layers dsm` prints a [dependency structure matrix](#dependency-structure-matrix)
  of the workspace crates as an HTML page. `--crate <name>` shows one
  crate's top-level modules. `--output <file>` writes the page to a file.
  CI uploads it as the `crate-dsm` artifact of the `Crate Layers` job.

The tool reads the manifests directly, so it needs neither `cargo metadata`
nor network access, and reads a git revision without touching the working
tree.

## When the check fails

An upward edge usually means a type lives in the wrong crate. The usual fix
is to move the type to the crate that owns the concept and let the higher
crate re-export it. Reclassify a crate only when its role is different from
what its layer describes. An `[[exception]]` with a `reason` accepts an edge
until the refactor that removes it lands; the check fails once the edge is
gone, so the list only shrinks.

## Module cycles

A module cycle is a set of a crate's top-level modules that all reach each
other through `crate::`, `super::`, `self::`, and child paths, including
through `use` re-exports. Test-only modules are left out, and an inline
`mod name { … }` counts as part of its parent. The tool reads paths from the
token stream without compiling, so a path built by a macro is not seen.

Break a cycle by moving what both sides need into a module that neither
depends on.

## Ratchets

Each of these tables in `crate-layers.toml` records a number per crate:

| Table | Number |
| --- | --- |
| `module_cycles` | The modules in the crate's largest module cycle. |
| `direct_dependencies` | The workspace crates the crate depends on through normal and build dependencies. |
| `leaked_dependencies` | The dependencies the crate [leaks](#leaks). |

A crate that is not listed accepts `0`. The check fails when a number grows,
and also when one shrinks until its entry is lowered, so the numbers only go
down. Raise a `direct_dependencies` entry only when the new dependency
belongs in that crate.

## Leaks

A crate leaks a workspace dependency when a public signature names one of
the dependency's items and the crate does not re-export it with `pub use`. A
caller that uses the signature then has to depend on that crate too. Fix a
leak by re-exporting the item from the crate that exposes it, or by keeping
it out of the public signature.

The public signatures are those of `pub` functions, fields, enums, traits,
type aliases, constants, and statics, outside test-only modules, that a
caller can reach. An item is reachable when a chain of `pub mod` leads to it
from the crate root, or when a reachable `pub use` re-exports it, its module,
or a glob over its module. A field or an `impl` method is reachable with the
type it belongs to, and a `pub use` covers a leak only when it is reachable.
The tool follows the imports of the item's own file or inline module, and
`extern crate … as …` aliases from the item's module and the crate root. It
misses:

- a path that reaches the dependency through another module of the crate;
- a name brought in by a glob import such as `use dep::*`;
- an item a `pub use` reaches through another `use` rather than through
  the module tree, which it counts as unreachable;
- the methods of an `impl` on a private type alias of a public type, which
  it counts as unreachable;
- a dependency renamed in `Cargo.toml`, which it looks for under its package
  name.

## Transitive budgets

A layer's `max_transitive_dependencies` caps how many workspace crates each
of its crates reaches through normal and build dependencies. It keeps the
lower layers cheap to build and to depend on. When a crate exceeds it, cut
the dependency that pulls in the most crates, usually by moving a type down to
the crate that owns it.

## Product boundary

The workspace holds two products, the pnpm CLI and the pnpr registry server.
A crate's product is the first component of its directory, `pnpm/` or
`pnpr/`, whatever the crate's name. `[product_boundary]` lists the crates
that a crate of the other product may depend on. The check covers every
dependency kind, dev-dependencies included, and fails:

- when a crate depends on a crate of the other product that is not listed;
- when a listed crate has no dependent in the other product, so the list
  only shrinks.

Before listing a crate, consider moving what both products need into a
crate that is already listed.

## Dependency structure matrix

The page lists the crates in rows and columns in the same order: by layer
from the bottom up, then by name. A mark in a cell means the row depends on
the column, so a dependency on a lower layer sits below the diagonal and an
upward dependency sits above it, highlighted. Lines separate the layers.

A cell's shade grows with how many commits changed both crates. A commit
changes a crate when it changes a file in the crate's directory. A pair
that is shaded but not marked changes together without depending on each
other, and the page lists those pairs below the matrix. Hover a cell for
its dependency kinds and commit counts.

`--commits` sets how many of the latest non-merge commits before `HEAD` the
page reads. A commit that changes more crates than `--max-changed`, such as
a rename across the workspace, counts for each crate but for no pair.
`just crate-layers dsm --help` prints the defaults.

With `--crate <name>`, the rows are the crate's top-level modules in name
order, and a mark means the module names the other. A commit changes a
module when it changes `src/<module>.rs` or a file under `src/<module>/`, so
a module whose file is set with `#[path]` shows no changes.
