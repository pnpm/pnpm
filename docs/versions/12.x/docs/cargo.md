---
id: cargo
title: Cargo dependencies
---

Added in: v12.4.0 (pnpm v12 only)

:::warning

Multi-ecosystem support is experimental. The settings and the layout it writes may change.

:::

pnpm can install the crates a Cargo workspace depends on, next to the npm packages in the same repository. One `pnpm install` resolves both graphs, fetches over one shared connection budget, and puts each crate in pnpm's content-addressable store, so a crate downloaded for one project is not downloaded again for the next.

Turn it on in `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
cargo:
  enabled: true
```

Then install as usual:

```sh
pnpm install
```

## Adding a crate

Prefix the crate name with `crate:`:

```sh
pnpm add crate:serde
pnpm add crate:serde@^1.0.200
```

pnpm writes the dependency into `Cargo.toml` without reformatting anything else in the file, and regenerates `Cargo.lock`. With no version requirement, the latest stable release is selected. A repository that has only a `Cargo.toml` needs no `package.json`: `pnpm add crate:` does not scaffold one.

## What an install does

1. Reads the workspace's `Cargo.toml`, including its members.
2. Resolves the graph against the sparse index and writes a deterministic `Cargo.lock`. When `Cargo.lock` is already there, it is used as-is.
3. Verifies each locked `.crate` archive against its checksum and unpacks it into pnpm's store.
4. Links the store slots into a Cargo [directory source](https://doc.rust-lang.org/cargo/reference/source-replacement.html) under `.pnpm/crates/crates-io`, generating the `.cargo-checksum.json` files Cargo expects.
5. Writes a source-replacement block into `.cargo/config.toml`, between the markers `# >>> pnpm-managed cargo sources >>>` and `# <<< pnpm-managed cargo sources <<<`. Anything you wrote outside those markers is left alone.

`cargo build` then compiles offline against the vendored sources. pnpm does not compile anything itself.

Crates are recorded in `Cargo.lock`, never in `pnpm-lock.yaml`. The two lockfiles stay independent.

`--lockfile-only`, `--frozen-lockfile`, and `--offline` mean for crates what they mean for npm packages: resolve without materializing, fail rather than change the lockfile, and refuse the network.

## Registries

```yaml title="pnpm-workspace.yaml"
cargo:
  enabled: true
registries:
  https://index.crates.io/:
    ecosystem: cargo
```

Declare the sparse index through [`registries`](./registries.md#ecosystem) with `ecosystem: cargo`. Without a declaration, pnpm uses crates.io. The registry a crate came from is recorded in `Cargo.lock`, and pnpm points Cargo's source replacement at that registry, so a local resolve and a [pnpr-accelerated](/pnpr/install-acceleration) one record the same source.

A dependency that names a third-party registry of its own (`registry = "..."` in `Cargo.toml`) is rejected: pnpm resolves one index per workspace.

### Authentication

Index and archive requests go through pnpm's URL-scoped credentials, the same map that authenticates npm registries. For crates.io, pnpm also reads `CARGO_REGISTRY_TOKEN` and `$CARGO_HOME/credentials.toml`. Cargo tokens keep their bare `Authorization` form and are only ever sent to the index host they belong to.

## Git-sourced crates

A crate pinned to a git revision, whether through `[patch.crates-io]` or a plain git dependency, is checked out at the commit `Cargo.lock` names and vendored the way `cargo vendor` lays a git package out: the package's own directory, a `.cargo-checksum.json` with a null `package`, and a source replacement for the git source. Vendored git packages get their own directory, `.pnpm/crates/git`, because a git package and a registry crate may share a name and version, and one directory source cannot hold both.

The store slot is keyed by the commit, so a second install links it without cloning again, offline included. `--frozen-lockfile` leaves the pinned revision untouched.

## Source overrides

pnpm honors root Cargo.toml `[patch]` and `[replace]` overrides when generating
`Cargo.lock` or adding, removing, and updating crates. Path overrides stay local.
Git overrides are pinned to the commit Cargo resolves and vendored for offline
builds.

Workspaces with source overrides or direct Git dependencies use Cargo for
resolution. This requires the default crates.io sparse index. With a custom index,
generate and commit `Cargo.lock` using Cargo before installing with pnpm.

Cargo resolution reads only settings for artifact dependencies and Rust version
resolution from checkout `.cargo` configuration. User Cargo home configuration
still applies. `--offline` requires the index and Git repositories needed for
resolution to be cached. `--frozen-lockfile` does not generate a missing
`Cargo.lock` or change an existing revision.

Git resolution permits the `file`, `git`, `http`, `https`, and `ssh` protocols.
When Cargo uses command-line Git fetching, pnpm preserves stricter protocol
settings from the workspace Git configuration and `GIT_ALLOW_PROTOCOL`. Unsupported transport helpers cannot run during lockfile resolution.

## Faster resolution through pnpr

With [`pnprServer`](/pnpr/install-acceleration) set, pnpm asks the server to resolve the Cargo graph instead of fetching one sparse-index file per crate. A server that does not answer for Cargo makes pnpm fall back to resolving locally.

## Settings

### cargo.enabled

* Default: **false**
* Type: **Boolean**

Whether `pnpm install` resolves and installs the workspace's Cargo dependencies.

### Cargo index

Since v12.5.0, declare a custom sparse index through [`registries`](./registries.md#ecosystem) with `ecosystem: cargo`. Without a declaration, pnpm uses `https://index.crates.io`. Only one Cargo index may be declared. `cargo.indexUrl` is no longer supported.

```yaml title="pnpm-workspace.yaml"
cargo:
  enabled: true
registries:
  https://crates.example.org/index/:
    ecosystem: cargo
```
