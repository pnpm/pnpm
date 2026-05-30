# pacquet

> **pacquet is under active development and not yet ready for production use.** See the [project roadmap](https://github.com/pnpm/pacquet/issues/299).

The official pnpm rewrite in Rust.

pacquet is a port of the [pnpm](https://github.com/pnpm/pnpm) CLI from TypeScript to Rust. It is not a new package manager and not a reimagining of pnpm. Its behavior, flags, defaults, error codes, file formats, and directory layout will match pnpm exactly.

## Installation

```sh
pnpm add -g pacquet
```

This package is a thin Node.js wrapper that dispatches to a platform-specific native binary for your operating system and architecture.
