# @pnpm/pnpr

A pnpm-compatible npm registry server, written in Rust.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under [`registry/`](https://github.com/pnpm/pnpm/tree/main/registry).

## Install

```sh
pnpm add -g @pnpm/pnpr
```

The wrapper resolves to the native binary published under
`@pnpm/pnpr.<platform>-<arch>` (e.g. `@pnpm/pnpr.linux-x64`).

## Usage

```sh
pnpr --help
```
