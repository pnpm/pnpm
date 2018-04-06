<p align="center">
  <img alt="supi - pnpm's installation engine" src="https://cdn.rawgit.com/pnpm/supi/master/logo.svg" width="200">
</p>

# supi

> Fast, disk space efficient installation engine. Used by [pnpm](https://github.com/pnpm/pnpm)

[![Status](https://travis-ci.org/pnpm/supi.svg?branch=master)](https://travis-ci.org/pnpm/supi "See test builds")
[![Windows build status](https://ci.appveyor.com/api/projects/status/18j52s5bdd71pjy5/branch/master?svg=true)](https://ci.appveyor.com/project/zkochan/supi/branch/master)

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

### `supi.installPkgs(pkgsToInstall, [options])`

Install packages.

**Arguments:**

* `pkgsToInstall` - *Object | String[]* - either an object that maps package names to version ranges or inputs usually passed to `npm install` (e.g., `foo@1.0.0`, `foo`).
* `options.storeController` - *Object* - required. An object that does all the manipulations with the store.
* `options.store` - *String* - required. Location of the store.
* `options.saveProd` - *Boolean* - package will appear in `dependencies`.
* `options.saveDev` - *Boolean* - package will appear in `devDependencies`.
* `options.saveOptional` - *Boolean* - package will appear in `optionalDependencies`.
* `options.saveExact` - *Boolean* - saved dependencies will be configured with an exact version rather than using npm's default semver range operator.
* `options.global` - *Boolean* - the packages will be installed globally rather than locally.
* `options.prefix` - *String* - the directory in which the installation will be performed. By default the `process.cwd()` value is used.
* `options.reporter` - *Function* - A function that listens for logs.
* `options.packageManager` - *Object* - The `package.json` of the package manager.
* `options.hooks` - *Object* - A property that contains installation hooks. Hooks are [documented separately](#hooks).
* `options.shrinkwrap` - *Boolean* - `true` by default. When `false`, ignores the `shrinkwrap.yaml` file and doesn't create/update one.
* `options.shrinkwrapOnly` - *Boolean* - `false` by default. When `true`, only updates `shrinkwrap.yaml` and `package.json` instead of checking `node_modules` and downloading dependencies.

**Returns:** a Promise

**Example:**

```js
const pnpm = require('pnpm')

pnpm.installPkgs({
  'is-positive': '1.0.0',
  'hello-world': '^2.3.1'
}, { saveDev: true })
```

### `supi.install([options])`

Install all modules listed as dependencies in `package.json`.

**Arguments:** (same as in named install and additionally)

* `options.production` - *Boolean* - `true` by default. If `true`, packages listed in `dependencies` will be installed.
* `options.development` - *Boolean* - `true` by default. If `true`, packages listed in `devDependencies` will be installed.
* `options.optional` - *Boolean* - Has the value of `options.production` by default. If `true`, packages listed in `optionalDependencies` will be installed.
  Can be `true` only when `options.production` is `true` as well.
* `options.frozenShrinkwrap` - *Boolean* - `false` by default. When `true`, shrinkwrap file is not generated and installation fails if an update is needed.
  With this option, a headless installation is performed. A headless installation is ~33% faster than a regular one because it skips
  dependencies resolution and peers resolution.
* `options.preferFrozenShrinkwrap` - *Boolean* - `true` by default. When `true`, a headless installation is performed if the shrinkwrap file
  is up-to-date with the `package.json` file.

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

### `supi.unlink([options])`

Unlinks all packages that were linked during development in a project. If the linked package is in `package.json` of the project,
it is installed after unlinking.

**Arguments:**

* `options.prefix` - *String* - by default `process.cwd()`. Path to the project.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.unlinkPkgs(pkgsToUnlink, [options])`

Unlinks the listed packages that were linked during development in a project. If the linked package is in `package.json` of the project,
it is installed after unlinking.

**Arguments:**

* `pkgsToUnlink` - *String[]* - the list of packages that have to be unlinked. If the passed in package is not an external link, then a warning is reported.
* `options.prefix` - *String* - by default `process.cwd()`. Path to the project.
* `options.reporter` - *Function* - A function that listens for logs.

### `supi.prune([options])`

Remove extraneous packages. Extraneous packages are packages that are not listed on the parent package's dependencies list.

**Arguments:**

* `options.production` - *Boolean* - by default `false`. If this property is `true`, prune will remove the packages specified in `devDependencies`.
* `options.prefix` - *String* - by default `process.cwd()`.
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

## Acknowledgements

Thanks to [Valentina Kozlova](https://github.com/ValentinaKozlova) for the supi logo

## License

[MIT](LICENSE)
