# @pnpm/core

> Fast, disk space efficient installation engine. Used by [pnpm](https://github.com/pnpm/pnpm)

## Install

Install it via npm.

```
pnpm add @pnpm/core
```

It also depends on `@pnpm/logger` version `1`, so install it as well via:

```
pnpm add @pnpm/logger@1
```

## API

### `mutateModules(importers, options)`

TODO

### `link(linkFromPkgs, linkToModules, [options])`

Create symbolic links from the linked packages to the target package's `node_modules` (and its `node_modules/.bin`).

**Arguments:**

* `linkFromPkgs` - *String[]* - paths to the packages that should be linked.
* `linkToModules` - *String* - path to the dependent package's `node_modules` directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `linkToGlobal(linkFrom, options)`

Create a symbolic link from the specified package to the global `node_modules`.

**Arguments:**

* `linkFrom` - *String* - path to the package that should be linked.
* `globalDir` - *String* - path to the global directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `linkFromGlobal(pkgNames, linkTo, options)`

Create symbolic links from the global `pkgName`s to the `linkTo/node_modules` folder.

**Arguments:**

* `pkgNames` - *String[]* - packages to link.
* `linkTo` - *String* - package to link to.
* `globalDir` - *String* - path to the global directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `storeStatus([options])`

Return the list of modified dependencies.

**Arguments:**

* `options.reporter` - *Function* - A function that listens for logs.

**Returns:** `Promise<string[]>` - the paths to the modified packages of the current project. The paths contain the location of packages in the store,
not in the projects `node_modules` folder.

### `storePrune([options])`

Remove unreferenced packages from the store.

## Hooks

Hooks are functions that can step into the installation process. All hooks can be provided as arrays to register multiple hook functions.

### `readPackage(pkg: Manifest, context): Manifest | Promise<Manifest>`

This hook is called with every dependency's manifest information.
The modified manifest returned by this hook is then used by `@pnpm/core` during installation.
An async function is supported.

**Arguments:**

* `pkg` - The dependency's package manifest.
* `context.log(message)` - A function to log debug messages.

**Example:**

```js
const { installPkgs } = require('@pnpm/core')

installPkgs({
  hooks: {
    readPackage: [readPackageHook]
  }
})

function readPackageHook (pkg, context) {
  if (pkg.name === 'foo') {
    context.log('Modifying foo dependencies')
    pkg.dependencies = {
      bar: '^2.0.0',
    }
  }
  return pkg
}
```

### `preResolution(context, logger): Promise<void>`

This hook is called after reading lockfiles but before resolving dependencies. It can modify lockfile objects.

**Arguments:**

* `context.wantedLockfile` - The lockfile from `pnpm-lock.yaml`.
* `context.currentLockfile` - The lockfile from `node_modules/.pnpm/lock.yaml`.
* `context.existsCurrentLockfile` - Boolean indicating if current lockfile exists.
* `context.existsNonEmptyWantedLockfile` - Boolean indicating if wanted lockfile exists and is not empty.
* `context.lockfileDir` - Directory containing the lockfile.
* `context.storeDir` - Location of the store directory.
* `context.registries` - Map of registry URLs.
* `logger.info(message)` - Log an informational message.
* `logger.warn(message)` - Log a warning message.

### `afterAllResolved(lockfile: Lockfile): Lockfile | Promise<Lockfile>`

This hook is called after all dependencies are resolved. It receives and returns the resolved lockfile object.
An async function is supported.

**Arguments:**

* `lockfile` - The resolved lockfile object that will be written to `pnpm-lock.yaml`.

### `adapters`

Custom adapters allow you to implement custom package resolution and fetching logic for new package identifier schemes (like `my-protocol:package-name`). Adapters are objects that can intercept and customize two phases of the installation process:

1. **Resolution phase**: Convert package descriptors (e.g., `foo@^1.0.0`) into resolutions
2. **Fetch phase**: Completely handle fetching for custom package types

**Adapter Interface:**

```typescript
interface Adapter {
  // Resolution phase
  canResolve?: (descriptor: WantedDependency) => boolean | Promise<boolean>
  resolve?: (descriptor: WantedDependency, opts: ResolveOptions) => ResolveResult | Promise<ResolveResult>

  // Fetch phase - complete fetcher replacement
  canFetch?: (pkgId: string, resolution: Resolution) => boolean | Promise<boolean>
  fetch?: (cafs: Cafs, resolution: Resolution, opts: FetchOptions, fetchers: Fetchers) => FetchResult | Promise<FetchResult>

  // Force resolution check
  shouldForceResolve?: (descriptor: WantedDependency, lockfileEntry?: PackageSnapshot) => boolean | Promise<boolean>
}
```

**Resolution Phase:**

* `canResolve(wantedDependency)` - Returns `true` if this adapter can resolve the given package descriptor
* `resolve(descriptor, opts)` - Resolves a package descriptor to a resolution. Should return an object with `id` and `resolution`

**Fetch Phase:**

* `canFetch(pkgId, resolution)` - Returns `true` if this adapter can handle fetching for the given resolution
* `fetch(cafs, resolution, opts, fetchers)` - Completely handles fetching the package contents. Receives the content-addressable file system (cafs), the resolution, fetch options, and pnpm's standard fetchers for delegation. Must return a FetchResult with the package files.

**Force Resolution:**

* `shouldForceResolve(descriptor, lockfileEntry)` - Called for each dependency an adapter can resolve. Return `true` to trigger full resolution of all packages (skipping the "Lockfile is up to date" optimization)

**Example - Reusing pnpm's fetcher utilities:**

```js
const { createLocalTarballFetcher, createDownloader } = require('@pnpm/tarball-fetcher')

const customAdapter = {
  canResolve: (wantedDependency) => {
    return wantedDependency.alias.startsWith('company-cdn:')
  },

  resolve: async (descriptor, opts) => {
    const actualName = wantedDependency.alias.replace('company-cdn:', '')
    const version = await fetchVersionFromCompanyCDN(actualName, wantedDependency.bareSpecifier)

    return {
      id: `company-cdn:${actualName}@${version}`,
      resolution: {
        type: '@company/cdn',
        cdnUrl: `https://cdn.company.com/packages/${actualName}/${version}.tgz`,
        cachedAt: Date.now(), // Custom metadata for shouldForceResolve
      },
    }
  },

  canFetch: (pkgId, resolution) => {
    return resolution.type === '@company/cdn'
  },

  fetch: async (cafs, resolution, opts, fetchers) => {
    // Delegate to pnpm's standard tarball fetcher
    // Transform the custom resolution to a standard tarball resolution
    const tarballResolution = {
      tarball: resolution.cdnUrl,
      integrity: resolution.integrity,
    }

    return fetchers.remoteTarball(cafs, tarballResolution, opts)
  },

  shouldForceResolve: (descriptor, lockfileEntry) => {
    // Force re-resolution if cached version is older than 1 day
    // This works because our resolve() method adds cachedAt to the resolution
    if (lockfileEntry?.resolution?.cachedAt) {
      return Date.now() - lockfileEntry.resolution.cachedAt > 86400000
    }
    return false
  },
}

installPkgs({
  hooks: {
    adapters: [customAdapter]
  }
})
```

**Delegating to Standard Fetchers:**

The `fetchers` parameter passed to `adapter.fetch` provides access to pnpm's standard fetchers for delegation:

* `fetchers.remoteTarball` - Fetch from remote tarball URLs
* `fetchers.localTarball` - Fetch from local tarball files
* `fetchers.gitHostedTarball` - Fetch from GitHub/GitLab/Bitbucket tarballs
* `fetchers.directory` - Fetch from local directories
* `fetchers.git` - Fetch from git repositories

See the test cases in `resolving/default-resolver/test/customResolver.ts` and `fetching/pick-fetcher/test/adapterFetch.ts` for complete working examples.

**Notes:**

* Multiple adapters can be registered; they are tried in order until one matches
* All adapter methods support both synchronous and asynchronous implementations
* Adapters are tried before pnpm's built-in resolvers (npm, git, tarball, etc.)
* The `fetch` method acts as a complete fetcher replacement, allowing custom package identifier schemes like `my-protocol:package`
* Adapters can delegate to pnpm's standard fetchers via the `fetchers` parameter to avoid reimplementing common fetch logic
* The `shouldForceResolve` hook allows fine-grained control over when packages should be re-resolved

## License

[MIT](LICENSE)
