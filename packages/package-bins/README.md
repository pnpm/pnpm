# @pnpm/package-bins

> Returns bins of a package

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/package-bins.svg)](https://www.npmjs.com/package/@pnpm/package-bins) [![Build Status](https://img.shields.io/travis/pnpm/package-bins/master.svg)](https://travis-ci.org/pnpm/package-bins)
<!--/@-->

## Installation

```sh
<pnpm|yarn|npm> add @pnpm/package-bins
```

## Usage

```ts
import getBinsFromPkg from '@pnpm/package-bins'

await getBinsFromPkg(path.resolve('package.json'), process.cwd())
//> [{name: 'bin-name', path: 'path-to-bin'}]
```

## API

### `getBinsFromPkg(packageJson, pathToPkg): Promise<Array<{name: string, path: string}>>`

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
