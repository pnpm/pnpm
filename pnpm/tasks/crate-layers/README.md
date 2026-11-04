# crate-layers

Checks the Rust workspace's crate dependency graph against the layers in
[`crate-layers.toml`](../../../crate-layers.toml).

- `just crate-layers check` fails when a crate's normal or build dependency is
  in a higher layer, when a crate is in no layer, or when an exception no
  longer matches an upward edge. It also fails when a crate's largest module
  cycle differs from its `module_cycles` entry. CI runs it in the
  `Crate Layers` job.
- `just crate-layers modules` lists the crates whose top-level modules form
  a cycle. `--crate <name>` prints one crate's cycle and the references inside
  it. `--baseline` prints a `module_cycles` table for the current state.
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

`module_cycles` in `crate-layers.toml` records each crate's largest cycle. A
crate that is not listed accepts none. The check fails when a cycle grows,
and also when one shrinks until its entry is lowered, so the numbers only go
down. Break a cycle by moving what both sides need into a module that
neither depends on.
