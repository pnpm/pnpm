# API

## `pnpm.installPkgs(pkgsToInstall, [options])`

Install packages.

**Arguments:**

* `pkgsToInstall` - *Object | String[]* - either an object that maps package names to version ranges or inputs usually passed to `npm install` (e.g., `foo@1.0.0`, `foo`).
* `options.save` - *Boolean* - package will appear in `dependencies`.
* `options.saveDev` - *Boolean* - package will appear in `devDependencies`.
* `options.saveOptional` - *Boolean* - package will appear in `optionalDependencies`.
* `options.saveExact` - *Boolean* - saved dependencies will be configured with an exact version rather than using npm's default semver range operator.
* `options.global` - *Boolean* - the packages will be installed globally rather than locally.
* `options.cwd` - *String* - the directory in which the installation will be performed. By default the `process.cwd()` value is used.
* `options.quiet` - *Boolean* - `false` by default. No output to the console.
* `options.cacheTTL` - *Number* - 1 day by default. The time (in seconds) during which HTTP requests are cached (except the ones that request tarballs).

**Returns:** a Promise

**Example:**

```js
const pnpm = require('pnpm')

pnpm.install({
  'is-positive': '1.0.0',
  'hello-world': '^2.3.1'
}, { save: true, quiet: true })
```

## `pnpm.install([options])`

Install all modules listed as dependencies in `package.json`.

**Arguments:** (same as in named install and additionally)

* `options.production` - *Boolean* - `false` by default or `true` when the `NODE_ENV` environment variable is set to `production`. Modules listed in `devDependencies` will not be installed.

## `pnpm.uninstall(pkgsToUninstall, [options])`

Uninstalls a package, completely removing everything pnpm installed on its behalf.

**Arguments:**

* `pkgsToUninstall` - *String[]* - the package names to be uninstalled.
* `options.save` - *Boolean* - the package will be removed from `dependencies`.
* `options.saveDev` - *Boolean* - the package will be removed from `devDependencies`.
* `options.saveOptional` - *Boolean* - the package will be removed from `optionalDependencies`.
* `options.global` - *Boolean* - the packages will be uninstalled globally.

## `pnpm.linkFromRelative(lintTo, [options])`

Create a symbolic link from the linked package to the current working directory's `node_modules` (and to the `node_modules/.bin`).

**Arguments:**

* `options.cwd` - *String* - by default `process.cwd()`.

## `pnpm.linkToGlobal([options])`

Create a symbolic link from the package in the current working directory to the global `node_modules`.

**Arguments:**

* `options.cwd` - *String* - by default `process.cwd()`.

## `pnpm.linkFromGlobal(pkgName, [options])`

Create a symbolic link to the specified package from the global `node_modules` to the current working directory's `node_modules`.

**Arguments:**

* `options.cwd` - *String* - by default `process.cwd()`.

## `pnpm.prune([options])`

Remove extraneous packages. Extraneous packages are packages that are not listed on the parent package's dependencies list.

**Arguments:**

* `options.production` - *Boolean* - by default `false`. If this property is `true`, prune will remove the packages specified in `devDependencies`.
* `options.cwd` - *String* - by default `process.cwd()`.

## `pnpm.prunePkgs(pkgs, [options])`

Remove extraneous packages specified in the `pkgs` arguments. Extraneous packages are packages that are not listed on the parent package's dependencies list.

**Arguments:**

* `pkgs` - *String[]* - prune only the specified packages.
* `options.production` - *Boolean* - by default `false`. If this property is `true`, prune will remove the packages specified in `devDependencies`.
* `options.cwd` - *String* - by default `process.cwd()`.
