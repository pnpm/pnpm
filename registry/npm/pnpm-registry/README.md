# pnpm-registry

A pnpm-compatible npm registry server, written in Rust.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under [`registry/`](https://github.com/pnpm/pnpm/tree/main/registry).

## Install

```sh
pnpm add -g pnpm-registry
```

The wrapper resolves to the native binary published under
`@pnpm/registry.<platform>-<arch>` (e.g. `@pnpm/registry.linux-x64`).

## Usage

```sh
pnpm-registry --help
```
