---
id: package_json
title: package.json
original_id: package_json
---

The manifest file of a package.

## Fields

### engines

You can specify the version of Node and pnpm that your stuff works on:

```json
{
    "engines": {
        "node": ">=10",
        "pnpm": ">=3"
    }
}
```

During local development, pnpm will always fail with an error message
when its version will not match the one in `engines.pnpm`.

Unless the user has set the `engine-strict` config flag, this field is advisory only and will only produce warnings when your package is installed as a dependency.

### peerDependenciesMeta

This field lists some extra information related to the dependencies listed in the `peerDependencies` field.

#### peerDependenciesMeta[pkg].optional

If true, the selected peer dependency will be marked as optional by the package manager and the consumer omitting it won't be reported as an error.

```json
{
    "peerDependencies": {
        "react-dom": "1"
    },
    "peerDependenciesMeta": {
        "react-dom": {
            "optional": true
        },
        // express is not specified in the peerDependencies field,
        // so pnpm will assume that any version of express is fine
        "express": {
            "optional": true
        }
    }
}
```

### publishConfig

Added in: v3.4.0

It is possible to override some fields in the manifest, before the package is packed.
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
