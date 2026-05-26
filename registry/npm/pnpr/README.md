# @pnpm/pnpr

A pnpm-compatible npm registry server, written in Rust. Plays a similar
role to [verdaccio](https://verdaccio.org/) — runs locally, proxies
public packages from a configured upstream, and lets you publish
private ones.

Lives in the [pnpm monorepo](https://github.com/pnpm/pnpm) under
[`registry/`](https://github.com/pnpm/pnpm/tree/main/registry).

## Install

```sh
pnpm add -g @pnpm/pnpr
```

The wrapper resolves to the native binary published under
`@pnpm/pnpr.<platform>-<arch>` (e.g. `@pnpm/pnpr.linux-x64`). Prebuilt
binaries are available for `linux-x64`, `linux-arm64`, `darwin-x64`,
`darwin-arm64`, `win32-x64`, and `win32-arm64`.

## Usage

Start the server with the bundled default config:

```sh
pnpr
```

It listens on `127.0.0.1:4873` and proxies `https://registry.npmjs.org/`
by default. Point a client at it with:

```sh
pnpm config set registry http://127.0.0.1:4873/
```

## CLI flags

| Flag | Description |
| --- | --- |
| `-c, --config <path>` | Path to a verdaccio-shaped YAML config. When omitted, the bundled default is used. |
| `--listen <addr>` | Address to bind to. Defaults to `127.0.0.1:4873`. |
| `--storage <path>` | Override the storage directory from the loaded config. |
| `--public-url <url>` | URL clients should use to reach the server, used when rewriting `dist.tarball` in served packuments. Defaults to `http://<listen>`. |
| `--packument-ttl-secs <n>` | Seconds before a cached packument is considered stale and refetched. |

Log level is controlled via the standard `RUST_LOG` environment
variable (e.g. `RUST_LOG=debug pnpr`).

## Configuration

`pnpr` uses a [verdaccio](https://verdaccio.org/docs/configuration)-shaped
YAML config. A minimal example:

```yaml
storage: ./storage

uplinks:
  npmjs:
    url: https://registry.npmjs.org/

packages:
  '@*/*':
    access: $all
    publish: $authenticated
    proxy: npmjs

  '**':
    access: $all
    publish: $authenticated
    proxy: npmjs
```

Pass it with `-c`:

```sh
pnpr -c ./pnpr.yaml
```
