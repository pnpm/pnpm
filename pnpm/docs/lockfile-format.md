---
id: lockfile-format
title: Lockfile format
---

This page specifies the contents of a `pnpm-lock.yaml` document: its keys,
the grammar of its package keys, and how its entries reference each other.
It is for tools that parse the lockfile. [Reading `pnpm-lock.yaml`](./lockfile.md)
covers how to split the file into documents, which documents a tool needs, and
which hash `integrity` pins.

This page covers the default `v9` format, including the boot and project
documents. See [`lockfile.format`](./settings/store.md#lockfileformat) for the
experimental `v13` format. The examples use made-up package names.

## Overview

A lockfile document records three layers:

| Key | Records | Keyed by |
| --- | --- | --- |
| [`importers`](#importers) | The direct dependencies of each project | Project directory |
| [`packages`](#packages) | Facts about a package version that do not depend on where it is installed | [Package key](#package-keys) without suffixes |
| [`snapshots`](#snapshots) | One installed instance of a package and the packages it is linked to | Full [package key](#package-keys) |

A project's dependency points to a snapshot. A snapshot's dependencies point to
other snapshots. Each snapshot has one `packages` entry, and several snapshots
can share it.

```yaml title="pnpm-lock.yaml"
lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false

importers:

  .:
    dependencies:
      plugin:
        specifier: ^2.0.0
        version: 2.1.0(host@5.0.0)
      host:
        specifier: ^5.0.0
        version: 5.0.0

packages:

  host@5.0.0:
    resolution: {integrity: sha512-...}

  plugin@2.1.0:
    resolution: {integrity: sha512-...}
    peerDependencies:
      host: '>=4'

  util@1.0.0:
    resolution: {integrity: sha512-...}

snapshots:

  host@5.0.0: {}

  plugin@2.1.0(host@5.0.0):
    dependencies:
      host: 5.0.0
      util: 1.0.0

  util@1.0.0: {}
```

## Top-level keys

The project document can contain the following keys.

| Key | Value |
| --- | --- |
| `lockfileVersion` | The format version, a string such as `'9.0'`. A lockfile that uses named registry package keys uses `12.0`. |
| `settings` | The settings the lockfile was resolved with. See [settings](#settings). |
| `catalogs` | The resolved [catalog](./catalogs.md) entries. See [catalogs](#catalogs). |
| `overrides` | The [`overrides`](./settings/dependency-resolution.md#overrides) in effect, in the order they are declared. |
| `packageExtensionsChecksum` | A hash of the [`packageExtensions`](./settings/dependency-resolution.md#packageextensions) in effect. |
| `pnpmfileChecksum` | A hash of the [pnpmfile](./pnpmfile.md), when it defines hooks. |
| `ignoredOptionalDependencies` | The [`ignoredOptionalDependencies`](./settings/dependency-resolution.md#ignoredoptionaldependencies) in effect. |
| `patchedDependencies` | Each [patched dependency](./cli/patch.md#patcheddependencies) selector mapped to the SHA-256 hex digest of its patch file. |
| `importers` | See [importers](#importers). |
| `packages` | See [packages](#packages). |
| `snapshots` | See [snapshots](#snapshots). |
| `time` | The publish time of each direct dependency, as `<name>@<version>: <ISO 8601 date>`. Written when [`resolutionMode`](./settings/other.md#resolutionmode) is `time-based`. |

pnpm compares the recorded `settings`, `catalogs`, `overrides`, checksums,
`ignoredOptionalDependencies`, and `patchedDependencies` with the current
configuration. If one differs, the lockfile is out of date, and
`pnpm install --frozen-lockfile` fails.

pnpm keeps top-level keys it does not define when it rewrites the file.
Readers can look up keys by name regardless of their order.

### settings

| Key | Written |
| --- | --- |
| `autoInstallPeers` | Always. The value of [`autoInstallPeers`](./settings/peer-dependencies.md#autoinstallpeers). |
| `excludeLinksFromLockfile` | Always. The value of [`excludeLinksFromLockfile`](./settings/store.md#excludelinksfromlockfile). |
| `dedupePeers` | As `true`, when [`dedupePeers`](./settings/peer-dependencies.md#dedupepeers) is on. |
| `addMissingPeerTypes` | As `true`, when [`addMissingPeerTypes`](./settings/peer-dependencies.md#addmissingpeertypes) is on. |
| `projectScopedOptionalPeers` | As `true`, when [`projectScopedOptionalPeers`](./settings/peer-dependencies.md#projectscopedoptionalpeers) takes effect. |
| `injectWorkspacePackages` | As `true`, when [`injectWorkspacePackages`](./workspaces.md#injectworkspacepackages) is on. |
| `peersSuffixMaxLength` | When [`peersSuffixMaxLength`](./settings/store.md#peerssuffixmaxlength) is not the default. |
| `integrityTarget` | As `tar`, when [`lockfile.integrityTarget`](./settings/store.md#lockfileintegritytarget) is `tar`. |
| `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents`, `linkWorkspacePackages` | When [`lockfile.includeResolutionSettings`](./settings/store.md#lockfileincluderesolutionsettings) is on. |

### catalogs

```yaml
catalogs:
  default:
    example-even:
      specifier: ^1.0.0
      version: 1.0.0
```

The keys are the catalog name (`default` for the default catalog) and then the
dependency name. `specifier` is the range in the catalog. `version` is what it
resolved to.

## importers

The keys are project directories, relative to the directory that holds the
lockfile, with `/` as the separator. The workspace root is `.`.

```yaml
importers:

  .:
    dependencies:
      my-example-utils:
        specifier: npm:example-utils@4.17.21
        version: example-utils@4.17.21
    devDependencies:
      example-odd:
        specifier: 3.0.1
        version: 3.0.1(patch_hash=580339f3...)
    optionalDependencies:
      example-file-watcher:
        specifier: 2.3.3
        version: 2.3.3

  packages/app:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:../lib
```

| Key | Value |
| --- | --- |
| `dependencies`, `devDependencies`, `optionalDependencies` | The project's direct dependencies of that type. Each key is the name the dependency is installed under in the project's `node_modules`. |
| `dependenciesMeta` | The project's `dependenciesMeta`, as declared. |
| `publishDirectory` | The project's `publishConfig.directory`. |
| `linkDirectory` | `false` when workspace links to this project ignore `publishDirectory`. |

Each dependency has two fields:

- `specifier`: the value from the project's `package.json`.
- `version`: what it resolved to, in one of the forms below.

| `version` form | Example | Meaning |
| --- | --- | --- |
| Version, with optional suffixes | `2.1.0(host@5.0.0)` | The snapshot key is `<dependency name>@<version>`. |
| `<name>@<version>` | `example-utils@4.17.21` | An aliased dependency. The snapshot key is the value itself. |
| `link:<path>` | `link:../lib` | A symlink to a directory, relative to the project directory. There is no snapshot. |
| `file:<path>`, with optional suffixes | `file:packages/lib(host@5.0.0)` | An injected workspace project or a local directory. The path is relative to the lockfile's directory. The snapshot key is `<dependency name>@<version>`. |
| URL | `https://codeload.github.com/...` | A package from a URL, such as a git host archive. The snapshot key is `<dependency name>@<version>`. |

A value is an alias when a package name comes before the version: it starts
with `@`, or it contains an `@` before any `(` or `:`.

### Boot document importers

The [boot document](./lockfile.md) has a single importer, `.`, with its own
dependency groups.

| Key | Value |
| --- | --- |
| `configDependencies` | Resolved [config dependencies](./config-dependencies.md). This map can be empty. |
| `packageManagerDependencies` | Resolved package manager dependencies from `packageManager` or `devEngines.packageManager`. Omitted when absent. |

Each group maps a dependency name to a `{specifier, version}` entry. The
`specifier` records the requested version or range. The `version` records
the selected version. These dependencies use the boot document's own
`packages` and `snapshots` maps.

## Package keys

The keys of `snapshots` are full package keys:

```text
<name>@<version>[(patch_hash=<hash>)][(<peer>)...]
```

| Part | Content |
| --- | --- |
| `<name>` | The package name from its manifest. |
| `<version>` | One of: a semver version; `<registry>:<version>` for a package from a [named registry](./settings/dependency-resolution.md#named-registries-in-the-lockfile); `runtime:<version>` for a [runtime](./cli/runtime.md); `file:<path>` for a local directory or tarball; or the URL of a non-registry package. |
| `(patch_hash=<hash>)` | Present when the package is patched. `<hash>` is the hex digest recorded in `patchedDependencies`. |
| `(<peer>)` | One group for each peer dependency that the package or its dependencies resolved from outside the package. See [peer suffixes](#peer-suffixes). |

The keys of `packages` are the same keys without the patch hash and the peer
suffixes. To find the `packages` entry of a snapshot, strip the trailing
parenthesized groups. Older lockfiles can also have a
[platform variant segment](#platform-variants-in-older-lockfiles).

```text
snapshots:  example-linter@9.39.5(example-color@7.2.0)
packages:   example-linter@9.39.5

snapshots:  example-odd@3.0.1(patch_hash=580339f3...)
packages:   example-odd@3.0.1
```

### Platform variants in older lockfiles

Older lockfiles can contain keys such as
`example-image@1.0.0/variants/linux-x64(example-host@2.0.0)`. The
`/variants/<tag>` segment appears after the registry version and before any
parenthesized suffixes. The metadata is under `example-image@1.0.0`. Packages
using `file:` paths or URLs can contain `/variants/` as part of the path.

For those lockfiles, pnpm used the target platform to choose a variant from
the package's `resolution.variants`. If that variant had its own snapshot,
pnpm used its dependency maps in place of the base snapshot's maps.
A graph reader covering all targets needs to visit the variant snapshots
as well. Current pnpm releases do not write these snapshots.

### Peer suffixes

A peer group normally contains the provider's full package key, including
its own suffixes. Groups can therefore nest.

```text
@example/lint-plugin@8.0.0(example-compiler@5.6.3)(example-linter@9.39.5(example-color@7.2.0))
```

With [`dedupePeers`](./settings/peer-dependencies.md#dedupepeers), or when
breaking a peer cycle, pnpm can use only the provider's name and version.
For a linked provider, the group contains the dependency name and an encoded
link path. For example, `example-host` linked as `link:packages/host` appears
as `(example-host@packages+host)`.

pnpm sorts the group contents and joins them with `)(`. This body includes any
nested parentheses and excludes the outermost pair. If its UTF-8 byte length
exceeds [`peersSuffixMaxLength`](./settings/store.md#peerssuffixmaxlength), pnpm
replaces the body with the first 32 hex characters of its SHA-256 digest.
pnpm wraps the resulting body in parentheses. Use the dependency maps to
follow resolved edges, including when a peer group is shortened or hashed.

A package that has no peer dependencies, and whose dependencies resolve none
from outside it, has no suffix. The same package version has one snapshot for
each distinct set of peers it was resolved with.

## packages

Facts about a package version that are the same for every snapshot of it.

```yaml
packages:

  example-logger@4.4.3:
    resolution: {integrity: sha512-...}
    engines: {node: '>=6.0'}
    peerDependencies:
      example-color: '*'
    peerDependenciesMeta:
      example-color:
        optional: true

  example-file-watcher@2.3.3:
    resolution: {integrity: sha512-...}
    engines: {node: ^8.16.0 || ^10.6.0 || >=11.0.0}
    os: [darwin]
```

| Key | Value |
| --- | --- |
| `resolution` | Where to fetch the package. See [resolution](#resolution). |
| `version` | The version from the manifest. Written when the key's `<version>` is a URL, a `file:` path, or a `runtime:` version. Not written for a directory. |
| `engines`, `cpu`, `os`, `libc` | Copied from the manifest. pnpm skips an optional package whose `cpu`, `os`, or `libc` does not match the current system. |
| `deprecated` | The registry's deprecation message. |
| `hasBin` | `true` when the package declares executables. |
| `bundledDependencies` | The package's bundled dependencies: a list of names, or `true`. |
| `peerDependencies` | The package's peer dependencies and their ranges. |
| `peerDependenciesMeta` | Which peer dependencies are optional. An optional peer is listed in both `peerDependencies` and `peerDependenciesMeta`. |

### resolution

| Form | Fields | Used for |
| --- | --- | --- |
| Registry | `integrity`, optional `revision` | A package from a registry. The tarball URL is derived from the registry and the package key. |
| Tarball | `tarball`, optional `integrity`, `gitHosted`, `path`, `revision` | A tarball at a URL or a local path. `gitHosted: true` marks a git host archive. `path` is a subdirectory inside the tarball. |
| `type: directory` | `directory` | A local directory, relative to the lockfile's directory. |
| `type: git` | `repo`, `commit`, optional `path` | A git repository cloned at `commit`. |
| `type: binary` | `url`, `integrity`, `bin`, `archive`, optional `prefix` | A binary archive, such as a runtime. |
| `type: variations` | `variants` | Platform-specific resolutions for a runtime or a registry package from an older lockfile. Each variant has a `resolution` and the `targets` it applies to. |

A `type` value other than those above is a custom resolution, defined by a
[resolver plugin](./pnpmfile.md). Its other fields belong to that resolver.

For `revision`, see [Resolutions that carry a revision](./lockfile.md#resolutions-that-carry-a-revision).
For what `integrity` pins, see [Which hash `integrity` pins](./lockfile.md#which-hash-integrity-pins).

## snapshots

One installed instance of a package. pnpm creates one directory in
`node_modules/.pnpm` for each snapshot, and links every dependency listed in
the snapshot into that directory's `node_modules`.

```yaml
snapshots:

  example-logger@4.4.3(example-color@7.2.0):
    dependencies:
      example-duration: 2.1.3
    optionalDependencies:
      example-color: 7.2.0

  example-file-watcher@2.3.3:
    optional: true

  example-utils@4.17.21: {}
```

| Key | Value |
| --- | --- |
| `dependencies` | Packages linked next to this one. See below. |
| `optionalDependencies` | The same, for edges that may fail to install. See below. |
| `transitivePeerDependencies` | See [transitivePeerDependencies](#transitivepeerdependencies). |
| `optional` | `true` when every path from a project to this snapshot goes through an optional dependency. pnpm then skips the package if it cannot be installed or built. |

A snapshot with none of these is written as `{}`.

### Dependencies of a snapshot

`dependencies` and `optionalDependencies` map the name a package is linked
under to a reference. They list both the package's own dependencies and the
providers of its resolved peer dependencies:

| Edge | Listed under |
| --- | --- |
| An entry of the manifest's `dependencies` | `dependencies` |
| An entry of the manifest's `optionalDependencies` | `optionalDependencies` |
| A resolved required peer dependency | `dependencies` |
| A resolved optional peer dependency | `optionalDependencies` |

A peer dependency that is not resolved is not listed. The list of a
snapshot's peers is in its `packages` entry.

A reference has one of these forms:

| Form | Example | Snapshot key |
| --- | --- | --- |
| Version, with optional suffixes | `example-duration: 2.1.3` | `<name>@<version>`, here `example-duration@2.1.3` |
| `<name>@<version>` | `example-width-cjs: example-width@4.2.3` | The value itself. The dependency is an alias. |
| `link:<path>` | `c: link:packages/c` | None. A symlink to a directory relative to the lockfile's directory. |
| `link:<root>/<path>` | `types: link:<root>/typings/css` | None. A symlink to a directory inside this package. |

The alias rule is the same as for [importers](#importers).

### transitivePeerDependencies

The names of peer dependencies that packages below this one declare, which
are not resolved inside this snapshot's subtree and are not this package's own
peer dependencies. Each name is one of:

- Resolved from outside this package. The provider then also appears in this
  snapshot's [peer suffix](#peer-suffixes).
- Not resolved anywhere.

```yaml
  container@3.2.2(buffer-impl@3.7.1):
    dependencies:
      fs-impl: 4.8.4(buffer-impl@3.7.1)
    transitivePeerDependencies:
      - abort-impl
      - buffer-impl
```

Here `fs-impl` declares the optional peers `buffer-impl` and `abort-impl`.
`buffer-impl` is resolved from outside `container`, so it is in `container`'s
suffix. `abort-impl` is not installed, so it is listed but not in the suffix.

### Fields of older lockfiles

Lockfiles written by earlier pnpm versions can carry `id` and `patched: true`
in a snapshot. pnpm reads them and does not write them.

## How a peer dependency ends up in the lockfile

A worked example. `container` depends on `fs-impl` and `url-impl`. `fs-impl`
declares `buffer-impl` as an optional peer dependency, and `url-impl` depends
on `buffer-impl`. This example uses workspace-wide optional peer selection
with `dedupePeerDependents: true` and `projectScopedOptionalPeers: false`.

```yaml
snapshots:

  container@3.2.2(buffer-impl@3.7.1):
    dependencies:
      fs-impl: 4.8.4(buffer-impl@3.7.1)
      url-impl: 2.5.6
    transitivePeerDependencies:
      - buffer-impl

  fs-impl@4.8.4(buffer-impl@3.7.1):
    optionalDependencies:
      buffer-impl: 3.7.1

  url-impl@2.5.6:
    dependencies:
      buffer-impl: 3.7.1

  buffer-impl@3.7.1: {}
```

1. `buffer-impl` is not a dependency of `container` or of any of its
   ancestors. It is installed elsewhere in the graph, as a dependency of
   `url-impl`. pnpm resolves an [optional peer dependency](./settings/peer-dependencies.md#optional-peer-dependencies)
   to such a version.
2. `fs-impl` gets the suffix `(buffer-impl@3.7.1)`, and `buffer-impl` is listed
   under its `optionalDependencies`, because the peer is optional.
3. `container` does not provide `buffer-impl` itself, so the suffix carries up
   to `container`, and `buffer-impl` is listed in its
   `transitivePeerDependencies`.
4. `url-impl` has a regular dependency on `buffer-impl`, so it has no suffix.

If `url-impl` is updated to a version that drops `buffer-impl` and pnpm finds
no provider for the optional peer, the keys of `fs-impl` and `container` lose
their suffix. An update targeting `url-impl` can leave `buffer-impl@3.7.1`
pinned in the lockfile. If pnpm still selects that pin, the suffix remains.

## Invariants

A lockfile written by pnpm satisfies these:

- Every `version` in `importers` and every reference in `snapshots` that is not
  a `link:` resolves to a key of `snapshots`.
- Every snapshot has a `packages` entry. Strip its parenthesized suffixes
  and any [legacy platform variant segment](#platform-variants-in-older-lockfiles)
  to find the metadata key.
- The maps under `importers`, `packages`, `snapshots`, and their dependency
  maps are sorted by key. `overrides` keeps its declared order.
