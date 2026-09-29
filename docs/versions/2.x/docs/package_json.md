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
[`bin`](https://github.com/stereobooster/package.json#bin),
[`main`](https://github.com/stereobooster/package.json#main),
[`exports`](https://nodejs.org/api/esm.html#esm_package_exports),
[`types` or `typings`](https://github.com/stereobooster/package.json#types),
[`module`](https://github.com/stereobooster/package.json#module),
[`browser`](https://github.com/stereobooster/package.json#browser),
[`esnext`](https://github.com/stereobooster/package.json#esnext),
[`es2015`](https://github.com/stereobooster/package.json#es2015),
[`unpkg`](https://github.com/stereobooster/package.json#unpkg-1) and
[`umd:main`](https://github.com/stereobooster/package.json#microbundle).

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

### pnpm.neverBuiltDependencies

Added in: v5.16.0

This field allows to ignore the builds of specific dependencies.

An example of the `"pnpm"."neverBuiltDependencies"` field:

```json
{
  "pnpm": {
    "neverBuiltDependencies": ["fsevents", "level"]
  }
}
```
