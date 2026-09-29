---
id: about-package-store
title: About the package store
original_id: about-package-store
---

A store is a folder that contains packages and information about projects that are using them.
The store does not include the `node_modules` folder of any of the packages, unless the package has
[bundled dependencies](https://docs.npmjs.com/files/package.json#bundleddependencies).

The store is immutable. Execution of modules from the store cannot remove/add files in the store,
because modules are executed in the context of the projects they are linked into.

## Store directory structure

Path structure: `<package source>/<package id>`. The path to a package in the store is the package's ID.

### Packages from npm-compatible registries

`<registry URL>/<package name>/<package version>`

E.g.:

```
registry.npmjs.org/gulp/2.1.0
registry.npmjs.org/@cycle/dom/14.1.0
registry.node-modules.io/@wmhilton/log/1.1.0
```

### Packages from Git

`<Git URL domain>/<Git path>/<commit hash>`

E.g.: `github.com/alexGugel/ied/b246270b53e43f1dc469df0c9b9ce19bb881e932`

## `store.json`

A file in the root of store that contains information about projects depending on specific packages from the store.

```json
{
  "/home/john_smith/src/ied": [
    "registry.npmjs.org/npm/3.10.2"
  ],
  "/home/john_smith/src/ied": [
    "registry.npmjs.org/arr-flatten/1.0.1",
    "registry.npmjs.org/byline/5.0.0",
    "registry.npmjs.org/cache-manager/2.2.0"
  ]
}
```
