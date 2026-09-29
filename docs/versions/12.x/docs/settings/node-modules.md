---
id: node-modules
title: "Node-Modules & Hoisting Settings"
sidebar_label: "node_modules & hoisting"
---

## Node-Modules Settings

### modulesDir

* Default: **node_modules**
* Type: **path**

The directory in which dependencies will be installed (instead of
`node_modules`).

### nodeLinker

* Default: **isolated**
* Type: **isolated**, **hoisted**, **pnp**

Defines what linker should be used for installing Node packages.

* **isolated** - dependencies are symlinked from a virtual store at `node_modules/.pnpm`.
* **hoisted** - a flat `node_modules` without symlinks is created. Same as the `node_modules` created by npm or Yarn Classic. One of Yarn's libraries is used for hoisting, when this setting is used. Legitimate reasons to use this setting:
  1. Your tooling doesn't work well with symlinks. A React Native project will most probably only work if you use a hoisted `node_modules`.
  1. Your project is deployed to a serverless hosting provider. Some serverless providers (for instance, AWS Lambda) don't support symlinks. An alternative solution for this problem is to bundle your application before deployment.
  1. If you want to publish your package with [`"bundledDependencies"`].
  1. If you are running Node.js with the [--preserve-symlinks] flag.
* **pnp** - no `node_modules`. Plug'n'Play is an innovative strategy for Node that is [used by Yarn Berry][pnp]. It is recommended to also set `symlink` setting to `false` when using `pnp` as
your linker.

[pnp]: https://yarnpkg.com/features/pnp
[--preserve-symlinks]: https://nodejs.org/api/cli.html#cli_preserve_symlinks
[`"bundledDependencies"`]: https://docs.npmjs.com/cli/v8/configuring-npm/package-json#bundleddependencies

### nodeExperimentalPackageMap

Added in: v11.8.0

* Default: **false**
* Type: **Boolean**

When `true`, pnpm writes `node_modules/.package-map.json` during isolated and hoisted installs and injects it into pnpm-managed Node.js script environments by adding Node's `--experimental-package-map` option to `NODE_OPTIONS`.

Nothing reads the map without this setting, so pnpm does not write one. An install that stops writing the map also removes one an earlier install left.

CLI and environment configuration use the kebab-case name `node-experimental-package-map`.

```yaml
nodeExperimentalPackageMap: true
```

### nodePackageMapType

Added in: v11.8.0

* Default: **standard**
* Type: **standard**, **loose**

Controls how `node_modules/.package-map.json` is generated.

* **standard** - only declared dependencies are available through the package map.
* **loose** - also maps packages that are reachable through the installed `node_modules` layout, which can allow undeclared hoisted dependencies to resolve.

CLI and environment configuration use the kebab-case name `node-package-map-type`.

```yaml
nodePackageMapType: loose
```

### symlink

* Default: **true**
* Type: **Boolean**

When `symlink` is set to `false`, pnpm creates a virtual store directory without
any symlinks. It is a useful setting together with `nodeLinker=pnp`.

### enableModulesDir

* Default: **true**
* Type: **Boolean**

When `false`, pnpm will not write any files to the modules directory
(`node_modules`). This is useful for when the modules directory is mounted with
filesystem in userspace (FUSE). There is an experimental CLI that allows you to
mount a modules directory with FUSE: [@pnpm/mount-modules].

[@pnpm/mount-modules]: https://www.npmjs.com/package/@pnpm/mount-modules

### virtualStoreDir

* Default: **node_modules/.pnpm**
* Types: **path**

The directory with links to the store. All direct and indirect dependencies of
the project are linked into this directory.

This is a useful setting that can solve issues with long paths on Windows. If
you have some dependencies with very long paths, you can select a virtual store
in the root of your drive (for instance `C:\my-project-store`).

Or you can set the virtual store to `.pnpm` and add it to `.gitignore`. This
will make stacktraces cleaner as paths to dependencies will be one directory
higher.

**NOTE:** the virtual store cannot be shared between several projects. Every
project should have its own virtual store (except for in workspaces where the
root is shared).

### virtualStoreDirMaxLength

* Default:
  * On Linux/macOS: **120**
  * On Windows: **60**
* Types: **number**

Sets the maximum allowed length of directory names inside the virtual store directory (`node_modules/.pnpm`). You may set this to a lower number if you encounter long path issues on Windows.

### virtualStoreOnly

Added in: v11.0.0

* Default: **false**
* Type: **Boolean**

When set to `true`, pnpm populates the virtual store without creating importer symlinks, hoisting, bin links, or running lifecycle scripts. This is useful for pre-populating a store (e.g., in Nix builds) without creating unnecessary project-level artifacts. `pnpm fetch` uses this mode internally.

### packageImportMethod

* Default: **auto**
* Type: **auto**, **hardlink**, **copy**, **clone**, **clone-or-copy**

Controls the way packages are imported from the store (if you want to disable symlinks inside `node_modules`, then you need to change the [nodeLinker] setting, not this one).

* **auto** - try the platform's cheap link tiers in order, falling back to
copying when none of them is possible. On Linux it tries hardlinking before
cloning; on macOS and Windows it tries cloning before hardlinking.
* **hardlink** - hard link packages from the store
* **clone-or-copy** - try to clone packages from the store. If cloning is not supported then fall back to copying
* **copy** - copy packages from the store
* **clone** - clone (AKA copy-on-write or reference link) packages from the store

Cloning is the best way to write packages to node_modules. It is the fastest way and safest way. When cloning is used, you may edit files in your node_modules and they will not be modified in the central content-addressable store.

Unfortunately, not all file systems support cloning. We recommend using a copy-on-write (CoW) file system (for instance, Btrfs instead of Ext4 on Linux) for the best experience with pnpm.

#### What `auto` tries first

`auto` is a per-platform order:

| Platform | Order |
|---|---|
| Linux | hardlink → clone → copy |
| macOS | clone → hardlink → copy |
| Windows | clone → hardlink → copy |

A reflink materializes a new inode and copies extent bookkeeping inside the
filesystem's metadata trees, where a hardlink is one directory entry. On btrfs
that difference roughly halves the time an install spends materializing
`node_modules` from a warm store, which is why pnpm reaches for the hardlink
first there. ext4 is unaffected — it never supported cloning, so `auto` already
hardlinked — and macOS keeps clone-first, since APFS `clonefile` is that
platform's cheap primitive.

If you edit files inside `node_modules`, ask for `clone` explicitly. Under a
hardlink, the file in `node_modules` *is* the file in the store, so editing it
changes every project that links the same package.

#### When a link succeeds but costs more than a copy

`auto` steps down a tier only when a link *fails*, never when one succeeds
slowly. Two setups produce links that succeed and are expensive, and on both
`packageImportMethod: copy` can be considerably faster.

Neither is a reason to reach for `copy` generally. On an ordinary install —
store and `node_modules` on the same everyday filesystem, no image layer in
between — copying is the slowest option and the one that uses the most space.
Measure both on the machine in question.

#### A store baked into a container image layer

Container runtimes assemble an image from stacked layers, usually with
overlayfs. A hardlink to a file that lives in a lower layer *succeeds* there,
but the file is copied into the writable upper layer first. An image that
ships a pre-populated pnpm store leaves that store in a lower layer, so the
first link to any store file copies that file up — and pnpm's "Packages are
hard linked from the content-addressable store" line stays accurate while the
install copies most of the store out anyway.

The cost is per file, not per link: a package linked into several projects is
copied up once and linked cheaply after that. It still lands on most of the
store, because an install links nearly every file it needs at least once. The
run below made 69k links and copied up 36.7k distinct files.

Measured with pnpm 12 (through `@pnpm/napi` 12.1.0) on a 13-project workspace,
2074 packages, store fully pre-populated so nothing was downloaded, timing
only the phase that writes `node_modules`. Rootless podman on the kernel
`overlay` storage driver with `metacopy` off, backed by btrfs, `--cpus=8`:

| `packageImportMethod` | Link phase | System time | Written |
| --- | --- | --- | --- |
| `auto` (hardlinking) | 2.1s | 8.0s | 36.7k files copied up, 366 MB |
| `copy` | 1.5s | 5.6s | 874 MB, nothing copied up |

Backing that overlay with btrfs is what keeps the gap small: a copy-up there
can share extents rather than duplicate them. On ext4 every copy-up is a full
data copy, and the gap widens — one project's CI, running this shape on an
ext4 boot disk, went from 38s to 16s by switching to `copy`.

Note that the `EXDEV` fallback below does not help here. A cross-layer
hardlink does not fail — it copies up and reports success — so `auto` has no
failure to step down on.

#### Network and virtualized filesystems

The same "succeeds, but slowly" shape appears when the store and
`node_modules` sit on the same filesystem and that filesystem services every
metadata operation over a round trip — a network share (NFS, SMB, Amazon EFS)
or a directory shared from a host into a VM or container (virtiofs,
gRPC-FUSE, 9p). Cloning, where the platform tries it first, fails there and
steps down; hardlinking then succeeds, so `auto` stays on it and the install
pays a round trip per file.
Copying reads and writes whole files instead of performing per-file metadata
operations the filesystem has to service remotely.

This only arises when both live on that filesystem. A store on a genuinely
different filesystem from `node_modules` cannot be hardlinked from at all —
the attempt fails with `EXDEV` — so `auto` already falls back to copying on
its own.

[nodeLinker]: #nodelinker

### modulesCacheMaxAge

* Default: **10080** (7 days in minutes)
* Type: **number**

The time in minutes after which orphan packages from the modules directory should be removed.
pnpm keeps a cache of packages in the modules directory. This boosts installation speed when
switching branches or downgrading dependencies.

### dlxCacheMaxAge

* Default: **1440** (1 day in minutes)
* Type: **number**

The time in minutes after which dlx cache expires.
After executing a dlx command, pnpm keeps a cache that omits the installation step for subsequent calls to the same dlx command.

### virtualStoreType

Added in: v11.23.0

* Default: **project**
* Type: **project**, **global**

Names where the virtual store lives — one store per project, or one store per machine.

```yaml
virtualStoreType: global
```

`project` is the default layout: every project gets its own virtual store inside `node_modules/.pnpm`. `global` is the [global virtual store](../global-virtual-store.md): a single store shared by every project on the machine, with each project's `node_modules` holding only symlinks into it.

This is the canonical spelling of [`enableGlobalVirtualStore`](#enableglobalvirtualstore), which keeps working; when a project sets both, `virtualStoreType` wins. It can also be set through the `PNPM_CONFIG_VIRTUAL_STORE_TYPE` env variable and read back with `pnpm config get virtualStoreType`.

The setting is independent of [`nodeLinker`](#nodelinker): `isolated` and `pnp` both work with either store type, and `hoisted` writes no virtual store at all, so it is unaffected.

### enableGlobalVirtualStore

Added in: v10.12.1

* Default: **false**
* Type: **Boolean**

:::note

Since v11.23.0, [`virtualStoreType`](#virtualstoretype) is the canonical spelling of this setting: `enableGlobalVirtualStore: true` is `virtualStoreType: global`. Both work, and `virtualStoreType` wins when a project sets both.

:::

:::note

Global installs (`pnpm add -g`) and `pnpm dlx` use the global virtual store by default.

:::

When enabled, `node_modules` contains only symlinks to a central virtual store, rather than to `node_modules/.pnpm`. By default, this central store is located at `<store-path>/links` (use `pnpm store path` to find `<store-path>`).

In the central virtual store, each package is hard linked into a directory whose name is the hash of its dependency graph. As a result, all projects on the system can symlink their dependencies from this shared location on disk. This approach is conceptually similar to how [NixOS manages packages], using dependency graph hashes to create isolated and shareable package directories in the Nix store.

> This should not be confused with the global content-addressable store. The actual package files are still hard linked from the content-addressable store—but instead of being linked directly into `node_modules/.pnpm`, they are linked into the global virtual store.

Using a global virtual store can significantly speed up installations when a warm cache is available. However, in CI environments (where caches are typically absent), it may slow down installation. If pnpm detects that it is running in CI, this setting is automatically disabled.

:::important

To support hoisted dependencies when using a global virtual store, pnpm relies on the `NODE_PATH` environment variable. This allows Node.js to resolve packages from the hoisted `node_modules` directory. Node.js does not respect `NODE_PATH` for ESM imports, though, so before v11.23.0 a dependency that imports a package **not declared in its own `package.json`** (which is considered bad practice) failed to resolve under ESM.

Since v11.23.0, every process pnpm spawns for the project — `pnpm run`, `pnpm exec`, lifecycle scripts, and the tools `pnpm dlx` runs — receives both `NODE_PATH` and a `NODE_OPTIONS` `--import` flag that registers a resolve hook restoring `NODE_PATH` lookups for ESM. Such imports now resolve for CommonJS and ESM alike, without the `@pnpm/plugin-esm-node-path` config dependency that used to be needed ([#9618](https://github.com/pnpm/pnpm/issues/9618)).

Two things fall outside that: a `node` process you start yourself rather than through pnpm, and a project that sets [`extendNodePath`](./other.md#extendnodepath) to `false`, which turns the whole `NODE_PATH` mechanism off and takes the resolve hook with it. Use [packageExtensions] to declare the missing dependencies if you need them to resolve in either case.

:::

[packageExtensions]: ./dependency-resolution.md#packageextensions
[NixOS manages packages]: https://nixos.org/guides/how-nix-works/

## Dependency Hoisting Settings

### hoist

* Default: **true**
* Type: **boolean**

When `true`, all dependencies are hoisted to `node_modules/.pnpm/node_modules`. This makes
unlisted dependencies accessible to all packages inside `node_modules`.

### hoistPattern

* Default: **['\*']**
* Type: **string[]**

Tells pnpm which packages should be hoisted to `node_modules/.pnpm/node_modules`. By
default, all packages are hoisted - however, if you know that only some flawed
packages have phantom dependencies, you can use this option to exclusively hoist
the phantom dependencies (recommended).

For instance:

```yaml
hoistPattern:
- "*eslint*"
- "*babel*"
```

You may also exclude patterns from hoisting using `!`.

For instance:

```yaml
hoistPattern:
- "*types*"
- "!@types/react"
```

In a workspace, the pattern is matched against the names of the workspace projects too, unless [`hoistWorkspacePackages`](#hoistworkspacepackages) is turned off.

### publicHoistPattern

* Default: **[]**
* Type: **string[]**

Unlike `hoistPattern`, which hoists dependencies to a hidden modules directory
inside the virtual store, `publicHoistPattern` hoists dependencies matching
the pattern to the root modules directory. Hoisting to the root modules
directory means that application code will have access to phantom dependencies,
even if they modify the resolution strategy improperly.

This setting is useful when dealing with some flawed pluggable tools that don't
resolve dependencies properly.

For instance:

```yaml
publicHoistPattern:
- "*plugin*"
```

Note: Setting `shamefullyHoist` to `true` is the same as setting
`publicHoistPattern` to `*`.

You may also exclude patterns from hoisting using `!`.

For instance:

```yaml
publicHoistPattern:
- "*types*"
- "!@types/react"
```

In a workspace, the pattern is matched against the names of the workspace projects too, unless [`hoistWorkspacePackages`](#hoistworkspacepackages) is turned off.

### hoistWorkspacePackages

* Default: **true**
* Type: **boolean**

When `true`, the projects of the workspace are hoisting candidates too, not only the packages that come from a registry.

[`hoistPattern`](#hoistpattern) and [`publicHoistPattern`](#publichoistpattern) then decide where each project lands, exactly as they do for a registry dependency. `publicHoistPattern` is checked first, so a project matching both patterns goes to the root only:

* A project whose name matches `publicHoistPattern` is symlinked into `<workspace_root>/node_modules`.
* A project whose name matches only `hoistPattern` is symlinked into `<workspace_root>/node_modules/.pnpm/node_modules`.
* A project whose name matches neither is not hoisted.

Because `hoistPattern` defaults to `['*']` and `publicHoistPattern` to `[]`, the default is that every workspace project is symlinked into `<workspace_root>/node_modules/.pnpm/node_modules`.

This is how a tool that resolves packages by name from the workspace root finds a project of the workspace. The usual case is ESLint loading a plugin that is developed in the same monorepo:

```yaml
publicHoistPattern:
  - "*eslint*"
```

With that setting, a workspace project named `eslint-plugin-acme` appears at `<workspace_root>/node_modules/eslint-plugin-acme`, even if no `package.json` lists it as a dependency.

Details worth knowing:

* A project is matched by the `name` in its `package.json`, not by its directory name.
* Every project of the workspace is a candidate, including a project that nothing else depends on. The workspace root project is never hoisted.
* The symlink points at the project's own directory in the workspace, so there is nothing to keep in sync.
* A direct dependency wins a name conflict. If a package with the same name is a direct dependency of any workspace project, that package is hoisted and the workspace project is not.
* The setting only adds candidates. When `hoistPattern` and `publicHoistPattern` are both empty, nothing is hoisted at all, workspace projects included. Setting [`hoist`](#hoist) to `false` empties `hoistPattern`.

### shamefullyHoist

* Default: **false**
* Type: **Boolean**

By default, pnpm creates a semistrict `node_modules`, meaning dependencies have
access to undeclared dependencies but modules outside of `node_modules` do not.
With this layout, most of the packages in the ecosystem work with no issues.
However, if some tooling only works when the hoisted dependencies are in the
root of `node_modules`, you can set this to `true` to hoist them for you.

### hoistingLimits

Added in: v11.5.0

* Default: **none**
* Type: **none**, **workspaces**, **dependencies**

Controls how far dependencies are hoisted when using `nodeLinker: hoisted`. This setting mirrors Yarn's `nmHoistingLimits`.

* **none** - hoist as far as possible (the default).
* **workspaces** - hoist only as far as each workspace package, preventing dependencies from being hoisted above the workspace package that depends on them.
* **dependencies** - hoist only up to each workspace package's direct dependencies, preventing transitive dependencies from being hoisted into the workspace package's `node_modules`.
