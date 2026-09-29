---
id: hooks
title: Hooks
original_id: hooks
---

pnpm allows to step directly into the installation process via special functions called *hooks*.
Hooks can be declared in a file called `pnpmfile.js`. `pnpmfile.js` should live in the root of the project.

## tl;dr

|Option|Meaning|
|--|--|
|`hooks.readPackage(pkg, context): pkg` | Allows to mutate every dependency's `package.json` |
|`hooks.afterAllResolved(lockfile, context): lockfile` | Is called after resolution stage. Allows to mutate the lockfile object. |

## `hooks.readPackage(pkg, context): pkg`

Allows to mutate every dependency's `package.json`.
An example of a `pnpmfile.js` that changes the dependencies field of a dependency:
You will need to delete the `pnpm-lock.yaml` if you have already resolved the dependency you want change.

```js
module.exports = {
  hooks: {
    readPackage
  }
}

function readPackage (pkg, context) {
  // Override the manifest of foo@1 after downloading it from the registry
  // Replace all dependencies with bar@2
  if (pkg.name === 'foo' && pkg.version.startsWith('1.')) {
    pkg.dependencies = {
      bar: '^2.0.0'
    }
    context.log('bar@1 => bar@2 in dependencies of foo')
  }

  // This will fix any dependencies on baz to 1.2.3
  if (pkg.dependencies && pkg.dependencies.baz === '*') {
    pkg.dependencies.baz = '1.2.3';
  }

  return pkg
}
```

### Arguments

* `pkg` - _Manifest_ - The manifest of the package. Either the response from the registry or the `package.json` content.
* `context.log(msg)` - _Function_ - Allows to log messages.

### Usage

#### Substitute a package with your fork

Lets' suppose you forked a package with an important fix and you want the fixed
version installed.

The following hook substitutes `resolve` with `@zkochan`'s fork.

```js
'use strict'
module.exports = {
  hooks: {
    readPackage
  }
}

function readPackage (pkg) {
  if (pkg.dependencies && pkg.dependencies.resolve) {
    pkg.dependencies.resolve = 'zkochan/node-resolve'
  }

  return pkg
}
```

#### Packages validation

You want only packages with MIT license in your `node_modules`? Check the licenses
and throw an exception if you don't like the package's license:

```js
'use strict'
module.exports = {
  hooks: {
    readPackage
  }
}

function readPackage (pkg) {
  if (pkg.license !== 'MIT') {
    throw new Error('Invalid license!')
  }

  return pkg
}
```

#### Renaming bins

You want to rename a package's bin? Just replace it:

```js
'use strict'
module.exports = {
  hooks: {
    readPackage
  }
}

function readPackage (pkg) {
  if (pkg.name === 'eslint') {
    pkg.bin = {jslint: pkg.bin}
  }

  return pkg
}
```

Now you can run `jslint fix` instead of `eslint fix`.

## `hooks.afterAllResolved(lockfile, context): lockfile`

Added in: v1.41.0

Is called after resolution stage. Allows to mutate the lockfile object.

### Arguments

* `lockfile` - _object_ - The object that is saved to `pnpm-lock.yaml`.
* `context.log(msg)` - _Function_ - Allows to log messages.

### Usage

```js
module.exports = {
  hooks: {
    afterAllResolved
  }
}

function afterAllResolved (lockfile, context) {
  // ...
  return lockfile
}
```
