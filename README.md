# pnpm-shrinkwrap

[![Build Status](https://travis-ci.org/pnpm/pnpm-shrinkwrap.svg?branch=master)](https://travis-ci.org/pnpm/pnpm-shrinkwrap)

> pnpm's shrinkwrap

Reads and writes the public (`shrinkwrap.yaml`) and private (`node_modules/.shrinkwrap.yaml`) shrinkwrap files of pnpm.
Shrinkwrap files are the state files of the `node_modules` installed via pnpm. They are like
the `package-lock.json` of npm or the `yarn.lock` of Yarn.

## Install

```
npm i pnpm-shrinkwrap
```

## API

### `read(pkgPath, opts) => Promise<Shrinkwrap>`

Reads the public `shrinkwrap.yaml` file from the root of the package.

#### Arguments

* `pkgPath` - *Path* - the path to the project
* `opts.ignoreIncompatible` - *Boolean* - `false` by default. If `true`, throws an error
if the shrinkwrap file format is not compatible with the current library.

### `readPrivate(pkgPath, opts) => Promise<Shrinkwrap>`

Same as `read()` but for the private shrinkwrap file at `node_modules/.shrinkwrap.yaml`.

### `write(pkgPath, shrinkwrap, privateShrinkwrap) => Promise<void>`

Writes the public private shrinkwrap files. When they are empty, removes them.

### `prune(shrinkwrap, package) => Promise<Shrinkwrap>`

Prunes a shrinkwrap file. Prunning means removing packages that are not referenced.

## License

[MIT](LICENSE)
