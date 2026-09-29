---
id: pnpmfile
title: .pnpmfile.mjs
---

pnpm lets you hook directly into the installation process via special functions
(hooks). Hooks can be declared in a file called `.pnpmfile.mjs` (ESM) or `.pnpmfile.cjs` (CommonJS).

By default, `.pnpmfile.mjs` should be located in the same directory as the
lockfile. For instance, in a [workspace](workspaces.md) with a shared lockfile,
`.pnpmfile.mjs` should be in the root of the monorepo.

## Hooks

### TL;DR

| Hook Function                                         | Process                                                    | Uses                                               |
|-------------------------------------------------------|------------------------------------------------------------|----------------------------------------------------|
| `hooks.readPackage(pkg, context): pkg`                | Called after pnpm parses the dependency's package manifest | Allows you to mutate a dependency's `package.json`. |
| `hooks.afterAllResolved(lockfile, context): lockfile` | Called after the dependencies have been resolved.          | Allows you to mutate the lockfile.                 |
| `hooks.beforePacking(pkg): pkg`                       | Called before creating a tarball during pack/publish       | Allows you to customize the published `package.json` |
| `resolvers`                                           | Called during package resolution.                          | Allows you to register custom package resolvers.  |
| `fetchers`                                            | Called during package fetching.                            | Allows you to register custom package fetchers.   |

### `hooks.readPackage(pkg, context): pkg | Promise<pkg>`

Allows you to mutate a dependency's `package.json` after parsing and prior to
resolution. These mutations are not saved to the filesystem, however, they will
affect what gets resolved in the lockfile and therefore what gets installed.

Note that you will need to delete the `pnpm-lock.yaml` if you have already
resolved the dependency you want to modify.

:::tip

If you need changes to `package.json` saved to the filesystem, you need to use the [`pnpm patch`] command and patch the `package.json` file.
This might be useful if you want to remove the `bin` field of a dependency for instance.

:::

#### Arguments

* `pkg` - The manifest of the package. Either the response from the registry or
the `package.json` content.
* `context` - Context object for the step. Method `#log(msg)` allows you to use
a debug log for the step.

#### Usage

Example `.pnpmfile.mjs` (changes the dependencies of a dependency):

```js
function readPackage(pkg, context) {
  // Override the manifest of foo@1.x after downloading it from the registry
  if (pkg.name === 'foo' && pkg.version.startsWith('1.')) {
    // Replace bar@x.x.x with bar@2.0.0
    pkg.dependencies = {
      ...pkg.dependencies,
      bar: '^2.0.0'
    }
    context.log('bar@1 => bar@2 in dependencies of foo')
  }

  // This will change any packages using baz@x.x.x to use baz@1.2.3
  if (pkg.dependencies.baz) {
    pkg.dependencies.baz = '1.2.3';
  }

  return pkg
}

export const hooks = {
  readPackage
}
```

#### Known limitations

Removing the `scripts` field from a dependency's manifest via `readPackage` will
not prevent pnpm from building the dependency. When building a dependency, pnpm
reads the `package.json` of the package from the package's archive, which is not
affected by the hook. In order to ignore a package's build, use the
[allowBuilds](settings/build.md#allowbuilds) field.

### `hooks.updateConfig(config): config | Promise<config>`

Added in: v10.8.0

Allows you to modify the configuration settings used by pnpm. This hook is most useful when paired with [configDependencies](config-dependencies), allowing you to share and reuse settings across different Git repositories.

For example, [@pnpm/plugin-better-defaults](https://github.com/pnpm/plugin-better-defaults) uses the `updateConfig` hook to apply a curated set of recommended settings.

#### Usage example

```js title=".pnpmfile.mjs"
export const hooks = {
  updateConfig (config) {
    return Object.assign(config, {
      enablePrePostScripts: false,
      optimisticRepeatInstall: true,
      resolutionMode: 'lowest-direct',
      verifyDepsBeforeRun: 'install',
    })
  }
}
```

### `hooks.afterAllResolved(lockfile, context): lockfile | Promise<lockfile>`

Allows you to mutate the lockfile output before it is serialized.

#### Arguments

* `lockfile` - The lockfile resolutions object that is serialized to
`pnpm-lock.yaml`.
* `context` - Context object for the step. Method `#log(msg)` allows you to use
a debug log for the step.

#### Usage example

```js title=".pnpmfile.mjs"
function afterAllResolved(lockfile, context) {
  // ...
  return lockfile
}

export const hooks = {
  afterAllResolved
}
```

#### Known Limitations

There are none - anything that can be done with the lockfile can be modified via
this function, and you can even extend the lockfile's functionality.

### `hooks.beforePacking(pkg): pkg | Promise<pkg>`

Added in: v10.28.0

Allows you to modify the `package.json` manifest before it is packed into a tarball during `pnpm pack` or `pnpm publish`. This is useful for customizing the published package without affecting your local development `package.json`.

Unlike `hooks.readPackage`, which modifies how dependencies are resolved during installation, `beforePacking` only affects the contents of the tarball that gets published.

#### Arguments

* `pkg` - The package manifest object that will be included in the published tarball.

#### Usage example

```js title=".pnpmfile.mjs"
function beforePacking(pkg) {
  // Remove development-only fields from published package
  delete pkg.devDependencies
  delete pkg.scripts.test

  // Add publication metadata
  pkg.publishedAt = new Date().toISOString()

  // Modify package exports for production
  if (pkg.name === 'my-package') {
    pkg.main = './dist/index.js'
  }

  return pkg
}

export const hooks = {
  beforePacking
}
```

:::note

The modifications made by this hook only affect the `package.json` inside the tarball. Your local `package.json` file remains unchanged.

:::

### `hooks.preResolution(options): Promise<void>`

This hook is executed after reading and parsing the lockfiles of the project, but before resolving dependencies. It allows modifications to the lockfile objects.

#### Arguments

* `options.existsCurrentLockfile` - A boolean that is true if the lockfile at `node_modules/.pnpm/lock.yaml` exists.
* `options.currentLockfile` - The lockfile object from `node_modules/.pnpm/lock.yaml`.
* `options.existsNonEmptyWantedLockfile` - A boolean that is true if the lockfile at `pnpm-lock.yaml` exists.
* `options.wantedLockfile` - The lockfile object from `pnpm-lock.yaml`.
* `options.lockfileDir` - The directory where the wanted lockfile is found.
* `options.storeDir` - The location of the store directory.
* `options.registries` - A map of scopes to registry URLs.

### `hooks.importPackage(destinationDir, options): Promise<string | undefined>`

:::warning Deprecated in v11.23.0

`importPackage` is deprecated and will be removed in the next major version. pnpm warns when a pnpmfile defines it, and defining it opts the installation out of the parallel package importer, making installation slower. If you rely on this hook, comment on [#14101](https://github.com/pnpm/pnpm/issues/14101).

:::

This hook allows to change how packages are written to `node_modules`. The return value is optional and states what method was used for importing the dependency, e.g.: clone, hardlink.

#### Arguments

* `destinationDir` - The destination directory where the package should be written.
* `options.disableRelinkLocalDirDeps`
* `options.filesMap`
* `options.force`
* `options.resolvedFrom`
* `options.keepModulesDir`

### `hooks.filterLog(log): boolean` {#hooksfilterlog}

`filterLog` returns `false` for a log entry to keep it out of pnpm's reporter
output. Use [`loglevel`](./settings/cli.md#loglevel) to choose the reporter's
overall verbosity.

### `hooks.fetchers`

:::danger Removed in v11.0.0

`hooks.fetchers` has been removed. Use top-level `fetchers` instead. See the [Custom Fetchers](#custom-fetchers) section for the new API.

:::


## Finders

Added in: v10.16.0

Finder functions are used with `pnpm list` and `pnpm why` via the `--find-by` flag.

Example:

```js title=".pnpmfile.mjs"
export const finders = {
  react17: (ctx) => {
    return ctx.readManifest().peerDependencies?.react === "^17.0.0"
  }
}
```

Usage:

```
pnpm why --find-by=react17
```

See [Finders] for more details.

[Finders]: ./finders.md

## Custom Resolvers and Fetchers

Added in: v11.0.0

Custom resolvers and fetchers allow you to implement custom package resolution and fetching logic for new package identifier schemes (like `my-protocol:package-name`). They are registered as top-level exports in `.pnpmfile.cjs`:

```js
module.exports = {
  resolvers: [customResolver1, customResolver2],
  fetchers: [customFetcher1, customFetcher2],
}
```

#### TypeScript Interfaces

```typescript
interface CustomResolver {
  canResolve?: (wantedDependency: WantedDependency) => boolean | Promise<boolean>
  resolve?: (wantedDependency: WantedDependency, opts: ResolveOptions) => ResolveResult | Promise<ResolveResult>
  shouldRefreshResolution?: (depPath: string, pkgSnapshot: PackageSnapshot) => boolean | Promise<boolean>
}

interface CustomFetcher {
  canFetch?: (pkgId: string, resolution: Resolution) => boolean | Promise<boolean>
  fetch?: (cafs: Cafs, resolution: Resolution, opts: FetchOptions, fetchers: Fetchers) => FetchResult | Promise<FetchResult>
}
```

### Custom Resolvers

Custom resolvers convert package descriptors (e.g., `foo@^1.0.0`) into resolutions that are stored in the lockfile.

#### Resolver Interface

A custom resolver is an object that can implement any combination of the following methods:

##### `canResolve(wantedDependency): boolean | Promise<boolean>`

Determines whether this resolver can resolve a given wanted dependency.

**Arguments:**
- `wantedDependency` - Object with:
  - `alias` - The package name or alias as it appears in package.json
  - `bareSpecifier` - The version range, git URL, file path, or other specifier

**Returns:** `true` if this resolver can handle the package, `false` otherwise. This determines whether `resolve` will be called.

##### `resolve(wantedDependency, opts): ResolveResult | Promise<ResolveResult>`

Resolves a wanted dependency to specific package metadata and resolution information.

**Arguments:**
- `wantedDependency` - The wanted dependency (same as `canResolve`)
- `opts` - Object with:
  - `lockfileDir` - Directory containing the lockfile
  - `projectDir` - The project root directory
  - `preferredVersions` - Map of package names to preferred versions

**Returns:** Object with:
- `id` - Unique package identifier (e.g., `'custom-pkg@1.0.0'`)
- `resolution` - Resolution metadata. This can be:
  - Standard resolution, e.g. `{ tarball: 'https://...', integrity: '...' }`
  - Custom resolution: `{ type: 'custom:cdn', url: '...' }`

Custom resolutions must be handled by a corresponding custom fetcher.

:::warning Custom Resolution Types

Custom resolutions must use the `custom:` prefix in their type field (e.g., `custom:cdn`, `custom:artifactory`) to differentiate them from pnpm's built-in resolution types.

:::

##### `shouldRefreshResolution(depPath, pkgSnapshot): boolean | Promise<boolean>`

Return `true` to trigger full resolution of all packages, skipping the "Lockfile is up to date" optimization. This is useful for implementing time-based cache invalidation or other custom re-resolution logic.

**Arguments:**
- `depPath` - The package identifier string (e.g., `lodash@4.17.21`)
- `pkgSnapshot` - The lockfile entry for this package, providing direct access to the resolution, dependencies, etc.

**Returns:** `true` to force re-resolution, `false` otherwise.

:::note

`shouldRefreshResolution` is skipped during frozen lockfile installs, as no resolution is allowed in that mode.

:::

### Custom Fetchers

Custom fetchers completely handle fetching for custom package types, downloading package contents from custom sources and storing them in pnpm's content-addressable file system.

#### Fetcher Interface

A custom fetcher is an object that can implement the following methods:

##### `canFetch(pkgId, resolution): boolean | Promise<boolean>`

Determines whether this fetcher can fetch a package with the given resolution.

**Arguments:**
- `pkgId` - The unique package identifier from the resolution phase
- `resolution` - The resolution object from a resolver's `resolve` method

**Returns:** `true` if this fetcher can handle fetching this package, `false` otherwise.

##### `fetch(cafs, resolution, opts, fetchers): FetchResult | CustomFetcherDelegation | Promise<FetchResult | CustomFetcherDelegation>`

Fetches package files and returns metadata about the fetched package.

**Arguments:**
- `cafs` - Content-addressable file system interface for storing files
- `resolution` - The resolution object (same as passed to `canFetch`)
- `opts` - Fetch options including:
  - `lockfileDir` - Directory containing the lockfile
  - `filesIndexFile` - Path for the files index
  - `onStart` - Optional callback when fetch starts
  - `onProgress` - Optional progress callback
- `fetchers` - Object containing pnpm's standard fetchers for delegation:
  - `remoteTarball` - Fetcher for remote tarballs
  - `localTarball` - Fetcher for local tarballs
  - `gitHostedTarball` - Fetcher for GitHub/GitLab/Bitbucket tarballs
  - `directory` - Fetcher for local directories
  - `git` - Fetcher for git repositories

**Returns:** either a fetch result, or a [delegation envelope](#delegating-to-the-built-in-fetchers).

A fetch result is an object with:
- `filesIndex` - Map of relative file paths to their physical locations. For remote packages, these are paths in pnpm's content-addressable store (CAFS). For local packages (when `local: true`), these are absolute paths to files on disk.
- `manifest` - Optional. The package.json from the fetched package. If not provided, pnpm will read it from disk when needed. Providing it avoids an extra file I/O operation and is recommended when you have the manifest data readily available (e.g., already parsed during fetch).
- `requiresBuild` - Boolean indicating whether the package has build scripts that need to be executed. Set to `true` if the package has `preinstall`, `install`, or `postinstall` scripts, or contains `binding.gyp` or `.hooks/` files. Standard fetchers determine this automatically using the manifest and file list.
- `local` - Optional. Set to `true` to load the package directly from disk without copying to pnpm's store. When `true`, `filesIndex` should contain absolute paths to files on disk, and pnpm will hardlink them to `node_modules` instead of copying. This is how the directory fetcher handles local dependencies (e.g., `file:../my-package`).

#### Delegating to the built-in fetchers

Added in: v11.12.0

Rather than fetching the package itself, a custom fetcher may hand the work back to pnpm. There are two ways to do this.

**Return a `{ delegate }` envelope.** Instead of a fetch result, return an object with a single `delegate` key holding the resolution pnpm should fetch instead. pnpm rewrites the package's resolution to that shape and runs its built-in fetch path on it:

```js title=".pnpmfile.cjs"
const customFetcher = {
  canFetch: (pkgId, resolution) => resolution.type === 'custom:url',
  fetch: (cafs, resolution) => ({
    delegate: {
      tarball: resolution.customUrl,
      integrity: resolution.integrity,
    },
  }),
}

module.exports = { fetchers: [customFetcher] }
```

The delegated resolution must be a complete, fetchable shape (for example `{ tarball, integrity }`). Delegation is single-step: a `delegate` that is itself custom-typed is rejected.

**Call `fetchers.*` directly.** The `fetchers` argument gives you pnpm's standard fetchers, so you can transform the resolution and invoke one yourself.

:::tip Prefer the envelope for portability

The `{ delegate }` envelope is the only delegation form that works in both pnpm and [pacquet](/blog/releases/12.0) (the Rust port of pnpm). pacquet invokes pnpmfile fetchers over IPC, where `cafs` and `fetchers` cannot exist and both arrive as `null`. A fetcher that should run on either stack must return the envelope rather than calling `fetchers.*`.

:::

#### Usage Examples

##### Basic Custom Resolver

This example shows a custom resolver that resolves packages from a custom registry:

```js title=".pnpmfile.cjs"
const customResolver = {
  // Only handle packages with @company scope
  canResolve: (wantedDependency) => {
    return wantedDependency.alias.startsWith('@company/')
  },

  resolve: async (wantedDependency, opts) => {
    // Fetch metadata from custom registry
    const response = await fetch(
      `https://custom-registry.company.com/${wantedDependency.alias}/${wantedDependency.bareSpecifier}`
    )
    const metadata = await response.json()

    return {
      id: `${metadata.name}@${metadata.version}`,
      resolution: {
        tarball: metadata.tarballUrl,
        integrity: metadata.integrity
      }
    }
  }
}

module.exports = {
  resolvers: [customResolver]
}
```

##### Custom Resolver and Fetcher with `shouldRefreshResolution`

This example shows a resolver and fetcher working together with a custom resolution type and time-based cache invalidation:

```js title=".pnpmfile.cjs"
const customResolver = {
  canResolve: (wantedDependency) => {
    return wantedDependency.alias.startsWith('company-cdn:')
  },

  resolve: async (wantedDependency, opts) => {
    const actualName = wantedDependency.alias.replace('company-cdn:', '')
    const version = await fetchVersionFromCompanyCDN(actualName, wantedDependency.bareSpecifier)

    return {
      id: `company-cdn:${actualName}@${version}`,
      resolution: {
        type: 'custom:cdn',
        cdnUrl: `https://cdn.company.com/packages/${actualName}/${version}.tgz`,
        cachedAt: Date.now(), // Custom metadata for shouldRefreshResolution
      },
    }
  },

  shouldRefreshResolution: (depPath, pkgSnapshot) => {
    // Check custom metadata stored in the resolution
    const cachedAt = pkgSnapshot.resolution?.cachedAt
    if (cachedAt && Date.now() - cachedAt > 24 * 60 * 60 * 1000) {
      return true // Re-resolve if cached more than 24 hours ago
    }
    return false
  },
}

const customFetcher = {
  canFetch: (pkgId, resolution) => {
    return resolution.type === 'custom:cdn'
  },

  fetch: async (cafs, resolution, opts, fetchers) => {
    // Delegate to pnpm's standard tarball fetcher
    const tarballResolution = {
      tarball: resolution.cdnUrl,
      integrity: resolution.integrity,
    }

    return fetchers.remoteTarball(cafs, tarballResolution, opts)
  },
}

module.exports = {
  resolvers: [customResolver],
  fetchers: [customFetcher],
}
```

##### Basic Custom Fetcher

This example shows a custom fetcher that fetches certain packages from a different source:

```js title=".pnpmfile.cjs"
const customFetcher = {
  canFetch: (pkgId, resolution) => {
    return pkgId.startsWith('@company/')
  },

  fetch: async (cafs, resolution, opts, fetchers) => {
    // Delegate to pnpm's tarball fetcher with modified URL
    const tarballResolution = {
      tarball: resolution.tarball.replace(
        'https://registry.npmjs.org/',
        'https://custom-registry.company.com/'
      ),
      integrity: resolution.integrity
    }

    return fetchers.remoteTarball(cafs, tarballResolution, opts)
  }
}

module.exports = {
  fetchers: [customFetcher]
}
```

##### Custom Resolution Type with Resolver and Fetcher

This example shows a custom resolver and fetcher working together with a custom resolution type:

```js title=".pnpmfile.cjs"
const customResolver = {
  canResolve: (wantedDependency) => {
    return wantedDependency.alias.startsWith('@internal/')
  },

  resolve: async (wantedDependency) => {
    return {
      id: `${wantedDependency.alias}@${wantedDependency.bareSpecifier}`,
      resolution: {
        type: 'custom:internal-directory',
        directory: `/packages/${wantedDependency.alias}/${wantedDependency.bareSpecifier}`
      }
    }
  }
}

const customFetcher = {
  canFetch: (pkgId, resolution) => {
    return resolution.type === 'custom:internal-directory'
  },

  fetch: async (cafs, resolution, opts, fetchers) => {
    // Delegate to pnpm's directory fetcher for local packages
    const directoryResolution = {
      type: 'directory',
      directory: resolution.directory
    }

    return fetchers.directory(cafs, directoryResolution, opts)
  }
}

module.exports = {
  resolvers: [customResolver],
  fetchers: [customFetcher]
}
```

#### Priority and Ordering

When multiple resolvers are registered, they are checked in order. The first resolver where `canResolve` returns `true` will be used for resolution. The same applies for fetchers: The first fetcher where `canFetch` returns `true` will be used during the fetch phase.

Custom resolvers are tried before pnpm's built-in resolvers (npm, git, tarball, etc.), giving you full control over package resolution.

#### Performance Considerations

`canResolve()`, `canFetch()`, and `shouldRefreshResolution()` should be cheap checks (ideally synchronous), as they're called for every dependency during resolution.

## Related Configuration

### ignorePnpmfile

* Default: **false**
* Type: **Boolean**

The pnpmfile will be ignored. Useful together with `--ignore-scripts` when you
want to make sure that no script gets executed during install.

### pnpmfile

* Default: **['.pnpmfile.mjs']**
* Type: **path[]**
* Example: **['.pnpm/.pnpmfile.mjs']**

The location of the local pnpmfile(s).

### globalPnpmfile

* Default: **null**
* Type: **path**
* Example: **~/.pnpm/global_pnpmfile.mjs**

The location of a global pnpmfile. A global pnpmfile is used by all projects
during installation.

:::note

It is recommended to use local pnpmfiles. Only use a global pnpmfile
if you use pnpm on projects that don't use pnpm as the primary package manager.

:::

[`pnpm patch`]: ./cli/patch.md
