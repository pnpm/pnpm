# crate-layers

Checks the Rust workspace's crate dependency graph against the layers in
[`crate-layers.toml`](../../../crate-layers.toml).

- `just crate-layers check` fails when a crate's normal or build dependency is
  in a higher layer, when a crate is in no layer, or when an exception no
  longer matches an upward edge. CI runs it in the `Crate Layers` job.
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
