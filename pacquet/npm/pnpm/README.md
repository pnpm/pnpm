# pnpm (Rust)

> **This is the Rust port of pnpm (formerly "pacquet"), published as an alpha.**
> It is under active development and **not yet ready for production use**. The
> stable, production-ready pnpm remains the TypeScript CLI on the `latest` tag.

This package ships the Rust rewrite of the [pnpm](https://github.com/pnpm/pnpm)
CLI under the `pnpm` name. It is a port, not a reimagining: its behavior, flags,
defaults, error codes, file formats, and directory layout match pnpm exactly.

## Installation

This release is published under the `alpha` dist-tag so it never replaces the
production pnpm on `latest`:

```sh
npm install -g pnpm@alpha
```

This package is a thin Node.js wrapper that dispatches to a platform-specific
native binary for your operating system and architecture. It provides the
`pnpm`, `pn`, `pnpx`, and `pnx` commands.

Prebuilt binaries are available for `linux-x64`, `linux-arm64`, `linux-x64-musl`,
`linux-arm64-musl`, `darwin-x64`, `darwin-arm64`, `win32-x64`, and `win32-arm64`.
