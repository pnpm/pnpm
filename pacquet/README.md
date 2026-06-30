# pacquet

> [!WARNING]
> **pacquet is under active development and not yet ready for production use.**

The official pnpm rewrite in Rust.

pacquet is the [pnpm](https://pnpm.io) CLI implemented in Rust — one of two parallel implementations of the same package manager, kept behaviorally identical (the same commands, flags, defaults, error codes, file formats, and directory layout). It is developed alongside the TypeScript pnpm CLI at near-complete feature parity, not as a downstream port that trails it.

## Roadmap

pacquet will become the installation engine of pnpm. The transition will happen in two phases.

### Phase 1: fetching and linking

pacquet replaces fetching and linking only. pnpm continues to create the lockfile, and pacquet does the rest. We expect this alone to make pnpm at least twice as fast in most scenarios. Shipping this phase is the current focus.

### Phase 2: resolution

pacquet also takes over dependency resolution.

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for development setup, debugging, testing, and benchmarking.

## Benchmark

![](https://pnpm.io/img/benchmarks/alotta-files-pnpm.svg)
