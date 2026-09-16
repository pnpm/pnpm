# pnpm v12 (pacquet)

The [pnpm](https://pnpm.io) v12 CLI is implemented in Rust. `pacquet` is its
in-repository package name; the published CLI and executable are named `pnpm`.

pnpm v12 is the target for new feature development. The TypeScript pnpm v11
CLI under `../pnpm11/` is maintained for bug fixes. Bugs present in both
versions are fixed in both implementations. Bugs present in only one version
are fixed in that version. New features are not backported to v11.

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for development setup, debugging, testing, and benchmarking.

## Cargo source overrides

With `cargo.enabled: true`, pnpm resolves root Cargo.toml `[patch]` and
`[replace]` overrides when generating Cargo.lock or editing crate dependencies.
Path overrides stay local, and Git overrides retain their pinned revisions.
Resolution uses Cargo and requires the default `cargo.indexUrl`. Workspaces
using a custom registry can install from an existing Cargo.lock.

Cargo resolution reads only settings for artifact dependencies and Rust version
resolution from checkout `.cargo` configuration. User Cargo home configuration
still applies. `--offline` requires the index and Git
repositories needed for resolution to be cached. Frozen installs do not generate
a missing Cargo.lock.

## Benchmark

![](https://pnpm.io/img/benchmarks/alotta-files-pnpm.svg)
