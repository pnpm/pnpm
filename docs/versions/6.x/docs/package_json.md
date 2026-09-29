---
id: package_json
title: package.json
---

The manifest file of a package. It contains all the package's metadata,
including dependencies, title, author, et cetera. This is a standard preserved
across all major Node.JS package managers, including pnpm.

## Fields

### engines

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

Unless the user has set the `engine-strict` config flag (see [.npmrc]), this
field is advisory only and will only produce warnings when your package is
installed as a dependency.

[.npmrc]: ./npmrc.md#engine-strict

### dependenciesMeta

#### dependenciesMeta.*.injected

Added in: v6.20.0

If this is set to true for a local dependency, the package will be hard linked to the modules directory, not symlinked.

For instance, the following `package.json` in a workspace will create a symlink to `button` in the `node_modules` directory of `card`:

```json
{
  "name": "card",
  "dependencies": {
    "button": "workspace:1.0.0"
  }
}
```

But what if `button` has `react` in its peer dependencies? If all projects in the monorepo use the same version of `react`, then no problem. But what if `button` is required by `card` that uses `react@16` and `form` with `react@17`? Without using `inject`, you'd have to choose a single version of `react` and install it as dev dependency of `button`. But using the `injected` field you can inject `button` to a package, and `button` will be installed with the `react` version of that package.

So this will be the `package.json` of `card`:

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

`button` will be hard linked into the dependencies of `card`, and `react@16` will be symlinked to the dependencies of `card/node_modules/button`.

And this will be the `package.json` of `form`:

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

`button` will be hard linked into the dependencies of `form`, and `react@17` will be symlinked to the dependencies of `form/node_modules/button`.

### peerDependenciesMeta

This field lists some extra information related to the dependencies listed in
the `peerDependencies` field.

#### peerDependenciesMeta.*.optional

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

### publishConfig

Added in: v3.4.0

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
* cpu
* os

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

#### publishConfig.executableFiles

Added in: v6.11.5

By default, for portability reasons, no files except those listed in the bin field will be marked as executable in the resulting package archive. The `executableFiles` field lets you declare additional fields that must have the executable flag (+x) set even if they aren't directly accessible through the bin field.

```json
{
  "publishConfig": {
    "executableFiles": [
      "./dist/shim.js"
    ]
  }
}
```

#### publishConfig.directory

Added in: v6.7.0

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

### pnpm.overrides

Added in: v5.10.1

This field allows you to instruct pnpm to override any dependency in the
dependency graph. This is useful to enforce all your packages to use a single
version of a dependency, backport a fix, or replace a dependency with a fork.

Note that the overrides field can only be set at the root of the project.

An example of the `"pnpm"."overrides"` field:

```json
{
  "pnpm": {
    "overrides": {
      "foo": "^1.0.0",
      "quux": "npm:@myorg/quux@^1.0.0",
      "bar@^2.1.0": "3.0.0",
      "qar@1>zoo": "2"
    }
  }
}
```

You may specify the package the overriden dependency belongs to by
separating the package selector from the dependency selector with a ">", for
example `qar@1>zoo` will only override the `zoo` dependency of `qar@1`, not for
any other dependencies.

### pnpm.packageExtensions

Added in: v6.9.0

The `packageExtensions` fields offer a way to extend the existing package definitions with additional information. For example, if `react-redux` should have `react-dom` in its `peerDependencies` but it has not, it is possible to patch `react-redux` using `packageExtensions`:

```json
{
  "pnpm": {
    "packageExtensions": {
      "react-redux": {
        "peerDependencies": {
          "react-dom": "*"
        }
      }
    }
  }
}
```

The keys in `packageExtensions` are package names or package names and semver ranges, so it is possible to patch only some versions of a package:

```json
{
  "pnpm": {
    "packageExtensions": {
      "react-redux@1": {
        "peerDependencies": {
          "react-dom": "*"
        }
      }
    }
  }
}
```

The following fields may be extended using `packageExtensions`: `dependencies`, `optionalDependencies`, `peerDependencies`, and `peerDependenciesMeta`.

A bigger example:

```json
{
  "pnpm": {
    "packageExtensions": {
      "express@1": {
        "optionalDependencies": {
          "typescript": "2"
        }
      },
      "fork-ts-checker-webpack-plugin": {
        "dependencies": {
          "@babel/core": "1"
        },
        "peerDependencies": {
          "eslint": ">= 6"
        },
        "peerDependenciesMeta": {
          "eslint": {
            "optional": true
          }
        }
      }
    }
  }
}
```

### pnpm.peerDependencyRules.ignoreMissing

Added in: v6.26.0

pnpm will not print warnings about missing peer dependencies from this list.

For instance, with the following configuration, pnpm will not print warnings if a dependency needs `react` but `react` is not installed:

```json
{
  "pnpm": {
    "peerDependencyRules": {
      "ignoreMissing": ["react"]
    }
  }
}
```

### pnpm.peerDependencyRules.allowedVersions

Added in: v6.26.0

Unmet peer dependency warnings will not be printed for peer dependencies of the specified range.

For instance, if you have some dependencies that need `react@16` but you know that they work fine with `react@17`, then you may use the following configuration:

```json
{
  "pnpm": {
    "peerDependencyRules": {
      "allowedVersions": {
        "react": "17"
      }
    }
  }
}
```

This will tell pnpm that any dependency that has react in its peer dependencies should allow `react` v17 to be installed.

### pnpm.neverBuiltDependencies

Added in: v5.16.0

This field allows to ignore the builds of specific dependencies.
The "preinstall", "install", and "postinstall" scripts of the listed packages will not be executed during installation.

An example of the `"pnpm"."neverBuiltDependencies"` field:

```json
{
  "pnpm": {
    "neverBuiltDependencies": ["fsevents", "level"]
  }
}
```

### pnpm.onlyBuiltDependencies

Added in: v6.32.0

A list of package names that are allowed to be executed during installation. If this field exists, only the listed packages will be able to run install scripts.

Example:

```json
{
  "pnpm": {
    "onlyBuiltDependencies": ["fsevents"]
  }
}
```
