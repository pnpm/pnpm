<p align="center">
  <img alt="supi - pnpm's installation engine" src="https://cdn.rawgit.com/pnpm/supi/master/logo.svg" width="200">
</p>

# supi

> Fast, disk space efficient installation engine. Used by [pnpm](https://github.com/pnpm/pnpm)

## Install

Install it via npm.

```
npm install supi
```

It also depends on `@pnpm/logger` version `1`, so install it as well via:

```
npm install @pnpm/logger@1
```

## API

### `supi.mutateModules(importers, options)`

TODO

### `supi.uninstall(pkgsToUninstall, [options])`

Uninstalls a package, completely removing everything pnpm installed on its behalf.

**Arguments:**

* `pkgsToUninstall` - *String[]* - the package names to be uninstalled.
* `options.saveProd` - *Boolean* - the package will be removed from `dependencies`.
* `options.saveDev` - *Boolean* - the package will be removed from `devDependencies`.
* `options.saveOptional` - *Boolean* - the package will be removed from `optionalDependencies`.
* `options.global` - *Boolean* - the packages will be uninstalled globally.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.link(linkFromPkgs, linkToNodeModules, [options])`

Create symbolic links from the linked packages to the target package's `node_modules` (and its `node_modules/.bin`).

**Arguments:**

* `linkFromPkgs` - *String[]* - paths to the packages that should be linked.
* `linkToNodeModules` - *String* - path to the dependent package's `node_modules` directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.linkToGlobal(linkFrom, options)`

Create a symbolic link from the specified package to the global `node_modules`.

**Arguments:**

* `linkFrom` - *String* - path to the package that should be linked.
* `globalPrefix` - *String* - path to the global directory.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.linkFromGlobal(pkgNames, linkTo, options)`

Create symbolic links from the global `pkgName`s to the `linkTo/node_modules` folder.

**Arguments:**

* `pkgNames` - *String[]* - packages to link.
* `linkTo` - *String* - package to link to.
* `globalPrefix` - *String* - path to the global directory.
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

### `afterAllResolved(shr: Shrinkwrap): Shrinkwrap`

This hook is called after all dependencies are resolved. It recieves and returns the resolved shrinkwrap object.

## Acknowledgements

Thanks to [Valentina Kozlova](https://github.com/ValentinaKozlova) for the supi logo

## License

[MIT](LICENSE)
