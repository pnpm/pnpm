---
id: package_json
title: package.json
---

The manifest file of a package. It contains all the package's metadata,
including dependencies, title, author, et cetera. This is a standard preserved
across all major Node.js package managers, including pnpm.

In addition to the traditional `package.json` format, pnpm also supports `package.json5` (via [json5]) and `package.yaml` (via [js-yaml]).

[json5]: https://www.npmjs.com/package/json5
[js-yaml]: https://www.npmjs.com/package/@zkochan/js-yaml

## engines

You can specify the version of Node and pnpm that your software works on:

```json
{
    "engines": {
        "node": ">=10",
        "pnpm": ">=3"
    }
}
```

During local development, pnpm will always fail with an error message
if its version does not match the one specified in the `engines` field.

Unless the user has set the `engineStrict` config flag (see [settings]), this
field is advisory only and will only produce warnings when your package is
installed as a dependency.

[settings]: ./settings.md#enginestrict

## engines.runtime

Added in: v10.21.0

Specifies the Node.js runtime required by a dependency. When declared, pnpm will automatically install the specified Node.js version.

```json
{
  "engines": {
    "runtime": {
      "name": "node",
      "version": "^24.11.0",
      "onFail": "download"
    }
  }
}
```

When a package declares a runtime:

1. **For CLI apps**: pnpm binds the CLI to the required Node.js version, ensuring it uses the correct runtime regardless of the globally installed Node.js instance.
2. **For packages with `postinstall` scripts**: The script executes using the specified Node.js version.

This is particularly useful for dependencies that require specific Node.js versions to function correctly.

## devEngines.runtime

Added in: v10.14

Allows to specify one or more JavaScript runtime engines used by the project. Supported runtimes are Node.js, Deno, and Bun.

For instance, here is how to add `node@^24.4.0` to your dependencies:

```json
{
  "devEngines": {
    "runtime": {
      "name": "node",
      "version": "^24.4.0",
      "onFail": "download"
    }
  }
}
```

You can also add multiple runtimes to the same `package.json`:

```json
{
  "devEngines": {
    "runtime": [
      {
        "name": "node",
        "version": "^24.4.0",
        "onFail": "download"
      },
      {
        "name": "deno",
        "version": "^2.4.3",
        "onFail": "download"
      }
    ]
  }
}
```

How it works:

1. `pnpm install` resolves your specified range to the latest matching runtime version.
1. The exact version (and checksum) is saved in the lockfile.
1. Scripts use the local runtime, ensuring consistency across environments.

## dependenciesMeta

Additional meta information used for dependencies declared inside `dependencies`, `optionalDependencies`, and `devDependencies`.

### dependenciesMeta.*.injected

If this is set to `true` for a dependency that is a local workspace package, that package will be installed by creating a hard linked copy in the virtual store (`node_modules/.pnpm`).

If this is set to `false` or not set, then the dependency will instead be installed by creating a `node_modules` symlink that points to the package's source directory in the workspace.  This is the default, as it is faster and ensures that any modifications to the dependency will be immediately visible to its consumers.

For example, suppose the following `package.json` is a local workspace package:

```json
{
  "name": "card",
  "dependencies": {
    "button": "workspace:1.0.0"
  }
}
```

The `button` dependency will normally be installed by creating a symlink in the `node_modules` directory of `card`, pointing to the development directory for `button`.

But what if `button` specifies `react` in its `peerDependencies`? If all projects in the monorepo use the same version of `react`, then there is no problem. But what if `button` is required by `card` that uses `react@16` and `form` that uses `react@17`? Normally you'd have to choose a single version of `react` and specify it using `devDependencies` of `button`. Symlinking does not provide a way for the `react` peer dependency to be satisfied differently by different consumers such as `card` and `form`.

The `injected` field solves this problem by installing a hard linked copies of `button` in the virtual store. To accomplish this, the `package.json` of `card` could be configured as follows:

```json
{
  "name": "card",
  "dependencies": {
    "button": "workspace:1.0.0",
    "react": "16"
  },
  "dependenciesMeta": {
    "button": {
      "injected": true
    }
  }
}
```

Whereas the `package.json` of `form` could be configured as follows:

```json
{
  "name": "form",
  "dependencies": {
    "button": "workspace:1.0.0",
    "react": "17"
  },
  "dependenciesMeta": {
    "button": {
      "injected": true
    }
  }
}
```

With these changes, we say that `button` is an "injected dependency" of `card` and `form`.  When `button` imports `react`, it will resolve to `react@16` in the context of `card`, but resolve to `react@17` in the context of `form`.

Because injected dependencies produce copies of their workspace source directory, these copies must be updated somehow whenever the code is modified; otherwise, the new state will not be reflected for consumers. When building multiple projects with a command such as `pnpm --recursive run build`, this update must occur after each injected package is rebuilt but before its consumers are rebuilt. For simple use cases, it can be accomplished by invoking `pnpm install` again, perhaps using a `package.json` lifecycle script such as `"prepare": "pnpm run build"` to rebuild that one project.  Third party tools such as [pnpm-sync](https://www.npmjs.com/package/pnpm-sync-lib) and [pnpm-sync-dependencies-meta-injected](https://www.npmjs.com/package/pnpm-sync-dependencies-meta-injected) provide a more robust and efficient solution for updating injected dependencies, as well as watch mode support.

## peerDependenciesMeta

This field lists some extra information related to the dependencies listed in
the `peerDependencies` field.

### peerDependenciesMeta.*.optional

If this is set to true, the selected peer dependency will be marked as optional
by the package manager. Therefore, the consumer omitting it will no longer be
reported as an error.

For example:
```json
{
    "peerDependencies": {
        "foo": "1"
    },
    "peerDependenciesMeta": {
        "foo": {
            "optional": true
        },
        "bar": {
            "optional": true
        }
    }
}
```

Note that even though `bar` was not specified in `peerDependencies`, it is
marked as optional. pnpm will therefore assume that any version of bar is fine.
However, `foo` is optional, but only to the required version specification.

## publishConfig

It is possible to override some fields in the manifest before the package is
packed.
The following fields may be overridden:

* [`bin`](https://github.com/stereobooster/package.json#bin)
* [`main`](https://github.com/stereobooster/package.json#main)
* [`exports`](https://nodejs.org/api/esm.html#esm_package_exports)
* [`types` or `typings`](https://github.com/stereobooster/package.json#types)
* [`module`](https://github.com/stereobooster/package.json#module)
* [`browser`](https://github.com/stereobooster/package.json#browser)
* [`esnext`](https://github.com/stereobooster/package.json#esnext)
* [`es2015`](https://github.com/stereobooster/package.json#es2015)
* [`unpkg`](https://github.com/stereobooster/package.json#unpkg-1)
* [`umd:main`](https://github.com/stereobooster/package.json#microbundle)
* [`typesVersions`](https://www.typescriptlang.org/docs/handbook/declaration-files/publishing.html#version-selection-with-typesversions)
* cpu
* os
* `engines` (Added in v10.22.0)

To override a field, add the publish version of the field to `publishConfig`.

For instance, the following `package.json`:

```json
{
    "name": "foo",
    "version": "1.0.0",
    "main": "src/index.ts",
    "publishConfig": {
        "main": "lib/index.js",
        "typings": "lib/index.d.ts"
    }
}
```

Will be published as:

```json
{
    "name": "foo",
    "version": "1.0.0",
    "main": "lib/index.js",
    "typings": "lib/index.d.ts"
}
```

### publishConfig.executableFiles

By default, for portability reasons, no files except those listed in the bin field will be marked as executable in the resulting package archive. The `executableFiles` field lets you declare additional files that must have the executable flag (+x) set even if they aren't directly accessible through the bin field.

```json
{
  "publishConfig": {
    "executableFiles": [
      "./dist/shim.js"
    ]
  }
}
```

### publishConfig.directory

You also can use the field `publishConfig.directory` to customize the published subdirectory relative to the current `package.json`.

It is expected to have a modified version of the current package in the specified directory (usually using third party build tools).

> In this example the `"dist"` folder must contain a `package.json`

```json
{
  "name": "foo",
  "version": "1.0.0",
  "publishConfig": {
    "directory": "dist"
  }
}
```

### publishConfig.linkDirectory

* Default: **true**
* Type: **Boolean**

When set to `true`, the project will be symlinked from the `publishConfig.directory` location during local development.

For example:

```json
{
  "name": "foo",
  "version": "1.0.0",
  "publishConfig": {
    "directory": "dist",
    "linkDirectory": true
  }
}
```
