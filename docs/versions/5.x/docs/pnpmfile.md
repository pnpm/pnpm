---
id: pnpmfile
title: pnpmfile.js
---

pnpm lets you hook directly into the installation process via special functions
(hooks). Hooks can be declared in a file called `pnpmfile.js`.

By default, `pnpmfile.js` should be located in the same directory as the
lockfile. For instance, in a [workspace](workspaces.md) with a shared lockfile,
`pnpmfile.js` should be in the root of the monorepo.

## Hooks

### TL;DR

| Hook Function                                         | Process                                                    | Uses                                               |
|-------------------------------------------------------|------------------------------------------------------------|----------------------------------------------------|
| `hooks.readPackage(pkg, context): pkg`                | Called after pnpm parses the dependency's package manifest | Allows you to mutate a dependency's `package.json` |
| `hooks.afterAllResolved(lockfile, context): lockfile` | Called after the dependencies have been resolved.          | Allows you to mutate the lockfile.                 |

### `hooks.readPackage(pkg, context): pkg`

Allows you to mutate a dependency's `package.json` after parsing and prior to
resolution. These mutations are not saved to the filesystem, however, they will
affect what gets resolved in the lockfile and therefore what gets installed.

Note that you will need to delete the `pnpm-lock.yaml` if you have already
resolved the dependency you want to modify.

#### Arguments

* `pkg` - The manifest of the package. Either the response from the registry or
the `package.json` content.
* `context` - Context object for the step. Method `#log(msg)` allows you to use
a debug log for the step.

#### Usage

Example `pnpmfile.js` (changes the dependencies of a dependency):

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

module.exports = {
  hooks: {
    readPackage
  }
}
```

#### Known limitations

Removing the `scripts` field from a dependency's manifest via `readPackage` will
not prevent pnpm from building the dependency. When building a dependency, pnpm
reads the `package.json` of the package from the package's archive, which is not
affected by the hook. In order to ignore a package's build, use the
[pnpm.neverBuiltDependencies](package_json.md#pnpmneverbuiltdependencies) field.

### `hooks.afterAllResolved(lockfile, context): lockfile`

Added in: v1.41.0

Allows you to mutate the lockfile output before it is serialized.

#### Arguments

* `lockfile` - The lockfile resolutions object that is serialized to
`pnpm-lock.yaml`.
* `context` - Context object for the step. Method `#log(msg)` allows you to use
a debug log for the step.

#### Usage example

```js title="pnpmfile.js"
function afterAllResolved(lockfile, context) {
  // ...
  return lockfile
}

module.exports = {
  hooks: {
    afterAllResolved
  }
}
```

#### Known Limitations

There are none - anything that can be done with the lockfile can be modified via
this function, and you can even extend the lockfile's functionality.

## Related Configuration

### ignore-pnpmfile

Added in: v1.25.0

* Default: **false**
* Type: **Boolean**

`pnpmfile.js` will be ignored. Useful together with `--ignore-scripts` when you
want to make sure that no script gets executed during install.

### pnpmfile

Added in: v1.39.0

* Default: **pnpmfile.js**
* Type: **path**
* Example: **.pnpm/pnpmfile.js**

The location of the local pnpmfile.

### global-pnpmfile

Added in: v1.39.0

* Default: **null**
* Type: **path**
* Example: **~/.pnpm/global_pnpmfile.js**

The location of a global pnpmfile. A global pnpmfile is used by all projects
during installation.

:::note

It is recommended to use local pnpmfiles. Only use a global pnpmfile
if you use pnpm on projects that don't use pnpm as the primary package manager.

:::
