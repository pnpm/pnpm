---
id: finders
title: Finders
---

Added in: v10.16.0

Finder functions let you **search your dependency graph** by any property of a package, not just its name.
They can be declared in [.pnpmfile.mjs] and used with [pnpm list] and [pnpm why].

[.pnpmfile.mjs]: ./pnpmfile.md
[pnpm list]: ./cli/list.md
[pnpm why]: ./cli/why.md

## Defining finder functions

Finder functions are declared in your project’s [.pnpmfile.mjs] file under the finders export.
Each function receives a context object and must return either:

* `true` → include this dependency in the results,
* `false` → skip it,
* or a `string` → include this dependency and print the string as additional info.

Example: a finder that matches any dependency with **React 17** in `peerDependencies`:

```js title=".pnpmfile.mjs"
export const finders = {
  react17: (ctx) => {
    return ctx.readManifest().peerDependencies?.react === "^17.0.0"
  }
}
```

### Finder context (ctx)

Each finder function receives a context object that describes the dependency node being visited.

|Field|Type/Example|Description|
|--|--|--|
|`name`|`"minimist"`|Package name.|
|`version`|`"1.2.8"`|Package version.|
|`readManifest()`|returns the `package.json` object|Load the package manifest (use this for fields like `peerDependencies`, `license`, `engines`, etc.).|

## Using finders

You can invoke a finder with the `--find-by=<functionName>` flag:

```sh
pnpm why --find-by=react17
```

Output:

```
@apollo/client 4.0.4
├── @graphql-typed-document-node/core 3.2.0
└── graphql-tag 2.12.6
```

## Returning extra metadata

A finder can also return a string. That string will be shown alongside the matched package in the output.

Example: print the package license:

```js title=".pnpmfile.mjs"
export const finders = {
  react17: (ctx) => {
    const manifest = ctx.readManifest()
    if (manifest.peerDependencies?.react === "^17.0.0") {
      return `license: ${manifest.license}`
    }
    return false
  }
}
```

Output:

```
@apollo/client 4.0.4
├── @graphql-typed-document-node/core 3.2.0
│   license: MIT
└── graphql-tag 2.12.6
    license: MIT
```

Other example use cases:
* Find all packages with a specific license.
* Detect packages requiring a minimum Node.js version.
* List all dependencies that expose binaries.
* Print funding URLs for all packages.
