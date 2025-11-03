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

### `preResolution(context, logger): Promise<result>`

This hook is called after reading lockfiles but before resolving dependencies. It can modify lockfile objects and force a full resolution by returning `{ forceFullResolution: true }`.

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

**Returns:** A promise resolving to `undefined` or an object with:
* `forceFullResolution` - When `true`, forces pnpm to re-resolve all dependencies.

### `afterAllResolved(lockfile: Lockfile): Lockfile | Promise<Lockfile>`

This hook is called after all dependencies are resolved. It receives and returns the resolved lockfile object.
An async function is supported.

**Arguments:**

* `lockfile` - The resolved lockfile object that will be written to `pnpm-lock.yaml`.

## License

[MIT](LICENSE)
