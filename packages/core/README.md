# @pnpm/core

> Fast, disk space efficient installation engine. Used by [pnpm](https://github.com/pnpm/pnpm)

## Install

Install it via npm.

```
<pnpm|yarn|npm> add @pnpm/core
```

It also depends on `@pnpm/logger` version `1`, so install it as well via:

```
<pnpm|yarn|npm> add @pnpm/logger@1
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

Hooks are functions that can step into the installation process.

### `readPackage(pkg: Manifest): Manifest | Promise<Manifest>`

This hook is called with every dependency's manifest information.
The modified manifest returned by this hook is then used by `@pnpm/core` during installation.
An async function is supported.

**Example:**

```js
const { installPkgs } = require('@pnpm/core')

installPkgs({
  hooks: {readPackage}
})

function readPackage (pkg) {
  if (pkg.name === 'foo') {
    pkg.dependencies = {
      bar: '^2.0.0',
    }
  }
  return pkg
}
```

### `afterAllResolved(lockfile: Lockfile): Lockfile | Promise<Lockfile>`

This hook is called after all dependencies are resolved. It recieves and returns the resolved lockfile object.
An async function is supported.

## License

[MIT](LICENSE)
