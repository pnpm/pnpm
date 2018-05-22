# pnpm-list

> List installed packages in a symlinked \`node_modules\`

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/pnpm-list.svg)](https://www.npmjs.com/package/pnpm-list) [![Build Status](https://img.shields.io/travis/pnpm/pnpm-list/master.svg)](https://travis-ci.org/pnpm/pnpm-list)
<!--/@-->

## Install

Install it via npm.

    npm install pnpm-list

## Usage

<!--@example('./example/index.js')-->
```js
'use strict'
const pnpmList = require('pnpm-list').default

pnpmList(__dirname, {depth: 2})
  .then(output => {
    console.log(output)
    //> pnpm-list@0.0.1 /home/zkochan/src/pnpm/pnpm-list/example
    //  └─┬ write-pkg@3.1.0
    //    ├─┬ sort-keys@2.0.0
    //    │ └── is-plain-obj@1.1.0
    //    └─┬ write-json-file@2.2.0
    //      ├── detect-indent@5.0.0
    //      ├── graceful-fs@4.1.11
    //      ├── make-dir@1.0.0
    //      ├── pify@2.3.0
    //      ├── sort-keys@1.1.2
    //      └── write-file-atomic@2.1.0
  })
```
<!--/@-->

## API

### default: `list(path, [opts]): Promise<string>`

Returns a string output similar to the `npm ls` but for [pnpm](github.com/pnpm/pnpm).

### Arguments

* `path` - *String* - path to the project
* `[opts.depth]` - *number* - `0` by default. Max display depth of the dependency tree.
* `[opts.only]` - *dev | prod* - `null` by default. Display only the dependency tree for packages in `devDependencies` or `dependencies`.
* `[opts.long]` - *Boolean* - `false` by default. If true, show extended information.
* `[opts.parseable]` - *Boolean* - `false` by default. Show parseable output instead of tree view.
* `[opts.alwaysPrintRootPackage]` - *Boolean* - `true` by default. Print the root package even if no dependencies found/matched.

### `forPackages(packages, path, [opts]): Promise<string>`

Returns a string output similar to the `npm ls [<@scope>/]<pkg> ...` but for [pnpm](github.com/pnpm/pnpm).

### Arguments

* `packages` - *String[]* - an array of `name@version-range` identifiers, which will limit the results to only the paths to the packages named.
* `path` - *String* - path to the project
* `[opts.depth]` - *number* - `0` by default. Max display depth of the dependency tree.
* `[opts.only]` - *dev | prod* - `null` by default. Display only the dependency tree for packages in `devDependencies` or `dependencies`.
* `[opts.long]` - *Boolean* - `false` by default. If true, show extended information.
* `[opts.parseable]` - *Boolean* - `false` by default. Show parseable output instead of tree view.
* `[opts.alwaysPrintRootPackage]` - *Boolean* - `true` by default. Print the root package even if no dependencies found/matched..

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
