# pnpm v12 (pacquet)

The latest stable [pnpm](https://pnpm.io) CLI, implemented in Rust. `pacquet` is
its in-repository package name; the published CLI and executable are named
`pnpm`.

pnpm v12 is the target for new feature development. The TypeScript pnpm v11
CLI under `../pnpm11/` is maintained for bug fixes. Bugs present in both
versions are fixed in both implementations, while new features are not
backported to v11.

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for development setup, debugging, testing, and benchmarking.

## Benchmark

![](https://pnpm.io/img/benchmarks/alotta-files-pnpm.svg)
