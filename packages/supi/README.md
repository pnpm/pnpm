<p align="center">
  <img alt="supi - pnpm's installation engine" src="https://cdn.rawgit.com/pnpm/supi/master/logo.svg" width="200">
</p>

# supi

> Fast, disk space efficient installation engine. Used by [pnpm](https://github.com/pnpm/pnpm)

## Install

Install it via npm.

```
<pnpm|yarn|npm> add supi
```

It also depends on `@pnpm/logger` version `1`, so install it as well via:

```
<pnpm|yarn|npm> add @pnpm/logger@1
```

## API

### `supi.mutateModules(importers, options)`

TODO

### `supi.link(linkFromPkgs, linkToModules, [options])`

Create symbolic links from the linked packages to the target package's `node_modules` (and its `node_modules/.bin`).

**Arguments:**

* `linkFromPkgs` - *String[]* - paths to the packages that should be linked.
* `linkToModules` - *String* - path to the dependent package's `node_modules` directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.linkToGlobal(linkFrom, options)`

Create a symbolic link from the specified package to the global `node_modules`.

**Arguments:**

* `linkFrom` - *String* - path to the package that should be linked.
* `globalDir` - *String* - path to the global directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.linkFromGlobal(pkgNames, linkTo, options)`

Create symbolic links from the global `pkgName`s to the `linkTo/node_modules` folder.

**Arguments:**

* `pkgNames` - *String[]* - packages to link.
* `linkTo` - *String* - package to link to.
* `globalDir` - *String* - path to the global directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.storeStatus([options])`

Return the list of modified dependencies.

**Arguments:**

* `options.reporter` - *Function* - A function that listens for logs.

**Returns:** `Promise<string[]>` - the paths to the modified packages of the current project. The paths contain the location of packages in the store,
not in the projects `node_modules` folder.

### `supi.storePrune([options])`

Remove unreferenced packages from the store.

## Hooks

Hooks are functions that can step into the installation process.

### `readPackage(pkg)`

This hook is called with every dependency's manifest information.
The modified manifest returned by this hook is then used by supi during installation.

**Example:**

```js
const supi = require('supi')

supi.installPkgs({
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

### `afterAllResolved(lockfile: Lockfile): Lockfile`

This hook is called after all dependencies are resolved. It recieves and returns the resolved lockfile object.

## Acknowledgements

Thanks to [Valentina Kozlova](https://github.com/ValentinaKozlova) for the supi logo

## License

[MIT](LICENSE)
