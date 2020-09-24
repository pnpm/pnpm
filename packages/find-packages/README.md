# find-packages

> Find all packages inside a directory

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/find-packages.svg)](https://www.npmjs.com/package/find-packages)
<!--/@-->

## Installation

```sh
<npm|yarn|pnpm> add find-packages
```

## Usage

```js
const path = require('path')
const findPkgs = require('find-packages')

findPkgs(path.join(__dirname, 'test', 'fixture'))
  .then(pkgs => console.log(pkgs))
  .catch(err => console.error(err))
  //> [ { path: '/home/zkochan/src/find-packages/test/fixture/pkg',
  //      manifest: { name: 'foo', version: '1.0.0' },
  //      writeProjectManifest: [AsyncFunction] } ]
```

## API

### `findPackages(dir, [opts])`

#### `dir`

The directory in which to search for packages.

#### `opts`

Parameters normally passed to [glob](https://www.npmjs.com/package/glob)

#### `opts.patterns`

Array of globs to use as package locations. For example: `['packages/**', 'utils/**']`.

#### `opts.ignore`

Patterns to ignore when searching for packages. By default: `**/node_modules/**`, `**/bower_components/**`, `**/test/**`, `**/tests/**`.

## License

MIT © [Zoltan Kochan](https://www.kochan.io)
