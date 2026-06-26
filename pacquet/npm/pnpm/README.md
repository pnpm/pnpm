# pnpm (Rust)

> **This is the Rust port of pnpm (formerly "pacquet"), published as an alpha.**
> It is under active development and **not yet ready for production use**. The
> stable, production-ready pnpm remains the TypeScript CLI on the `latest` tag.

This package ships the Rust rewrite of the [pnpm](https://github.com/pnpm/pnpm)
CLI under the `pnpm` name. It is a port, not a reimagining: its behavior, flags,
defaults, error codes, file formats, and directory layout match pnpm exactly.

## Installation

This release is published under the `next-12` dist-tag so it never replaces the
production pnpm on `latest`:

```sh
npm install -g pnpm@next-12
```

On install, a preinstall script replaces the package's placeholder bin with the
platform-specific native binary for your operating system and architecture, so
`pnpm` runs the binary directly with no Node.js startup cost. It provides the
`pnpm`, `pn`, `pnpx`, and `pnx` commands. (Because the binary is linked by a
build script, installing with build scripts disabled — `--ignore-scripts`, or
pnpm's/Bun's default — leaves the placeholder in place until pnpm is
allow-listed.)

Prebuilt binaries are available for `linux-x64`, `linux-arm64`, `linux-x64-musl`,
`linux-arm64-musl`, `darwin-x64`, `darwin-arm64`, `win32-x64`, and `win32-arm64`.
