# dependencies-hierarchy

> Creates a dependencies hierarchy for a symlinked \`node_modules\`

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/dependencies-hierarchy.svg)](https://www.npmjs.com/package/dependencies-hierarchy) [![Build Status](https://img.shields.io/travis/pnpm/dependencies-hierarchy/master.svg)](https://travis-ci.org/pnpm/dependencies-hierarchy)
<!--/@-->

A symlinked `node_modules` is created when installing using [pnpm](https://github.com/pnpm/pnpm).

## Install

Install it via npm.

    npm install dependencies-hierarchy

## Usage

<!--@example('./example/index.js')-->
```js
'use strict'
const hierarchyForPackages = require('dependencies-hierarchy').forPackages

hierarchyForPackages(['graceful-fs', {name: 'pify', range: '2'}], __dirname, {depth: 2})
  .then(tree => {
    console.log(JSON.stringify(tree, null, 2))
    //> [
    //    {
    //      "dependencies": [
    //        {
    //          "dependencies": [
    //            {
    //              "pkg": {
    //                "name": "graceful-fs",
    //                "path": "registry.npmjs.org/graceful-fs/4.1.11",
    //                "version": "4.1.11"
    //              },
    //              "searched": true
    //            },
    //            {
    //              "pkg": {
    //                "name": "pify",
    //                "path": "registry.npmjs.org/pify/2.3.0",
    //                "version": "2.3.0"
    //              },
    //              "searched": true
    //            }
    //          ],
    //          "pkg": {
    //            "name": "write-json-file",
    //            "path": "registry.npmjs.org/write-json-file/2.2.0",
    //            "version": "2.2.0"
    //          }
    //        }
    //      ],
    //      "pkg": {
    //        "name": "write-pkg",
    //        "path": "registry.npmjs.org/write-pkg/3.1.0",
    //        "version": "3.1.0"
    //      }
    //    }
    //  ]
  })
```
<!--/@-->

## API

### default: `dependenciesHierarchy(projectPath, [opts]): Promise<Hierarchy>`

Creates a dependency tree for a project's `node_modules`.

#### Arguments:

- `projectPath` - _String_ - The path to the project.
- `[opts.depth]` - _Number_ - 0 by default. How deep should the `node_modules` be analyzed.
- `[opts.only]` - _'dev' | 'prod'_ - Optional. If set to `dev`, then only packages from `devDependencies` are analyzed.
  If set to `prod`, then only packages from `dependencies` are analyzed.

### `forPackages(packageSelectors, projectPath, [opts]): Promise<Hierarchy>`

Creates a dependency tree for a project's `node_modules`. Limits the results to only the paths to the packages named.

#### Arguments:

- `packageSelectors` - _(string | {name: string, version: string})\[]_ - An array that consist of package names or package names and version ranges.
  E.g. `['foo', {name: 'bar', version: '^2.0.0'}]`.
- `projectPath` - _String_ - The path to the project
- `[opts.depth]` - _Number_ - 0 by default. How deep should the `node_modules` be analyzed.
- `[opts.only]` - _'dev' | 'prod'_ - Optional. If set to `dev`, then only packages from `devDependencies` are analyzed.
  If set to `prod`, then only packages from `dependencies` are analyzed.

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
