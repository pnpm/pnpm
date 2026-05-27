# pacquet

> [!WARNING]
> **pacquet is under active development and not yet ready for production use.**

The official pnpm rewrite in Rust.

pacquet is a port of the [pnpm](https://github.com/pnpm/pnpm) CLI from TypeScript to Rust. It is not a new package manager and not a reimagining of pnpm. Its behavior, flags, defaults, error codes, file formats, and directory layout will match pnpm exactly.

## Roadmap

pacquet will become the installation engine of pnpm. The transition will happen in two phases.

### Phase 1: fetching and linking

pacquet replaces fetching and linking only. pnpm continues to create the lockfile, and pacquet does the rest. We expect this alone to make pnpm at least twice as fast in most scenarios. Shipping this phase is the current focus.

### Phase 2: resolution

pacquet also takes over dependency resolution.

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for development setup, debugging, testing, and benchmarking.

## Benchmark

![](https://pnpm.io/img/benchmarks/alotta-files-pnpm.svg)
