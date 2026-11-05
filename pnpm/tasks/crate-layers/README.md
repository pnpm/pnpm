# crate-layers

Checks the Rust workspace's crate dependency graph against the layers in
[`crate-layers.toml`](../../../crate-layers.toml).

- `just crate-layers check` fails when a crate's normal or build dependency is
  in a higher layer, when a crate is in no layer, or when an exception no
  longer matches an upward edge. It also fails when a
  [ratchet](#ratchets) value differs from its entry, and when a crate
  exceeds its layer's [transitive budget](#transitive-budgets). CI runs it in
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
type aliases, constants, and statics, outside test-only modules. The tool
reads `pub` alone, so a `pub` item in a private module counts. It follows
the imports of the item's own file or inline module, and `extern crate … as …`
aliases from the item's module and the crate root. It misses:

- a path that reaches the dependency through another module of the crate;
- a name brought in by a glob import such as `use dep::*`;
- a dependency renamed in `Cargo.toml`, which it looks for under its package
  name.

## Transitive budgets

A layer's `max_transitive_dependencies` caps how many workspace crates each
of its crates reaches through normal and build dependencies. It keeps the
lower layers cheap to build and to depend on. When a crate exceeds it, cut
the dependency that pulls in the most crates, usually by moving a type down to
the crate that owns it.
