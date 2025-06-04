# @pnpm/package-bins

> Returns bins of a package

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/package-bins.svg)](https://www.npmjs.com/package/@pnpm/package-bins)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/package-bins
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

[MIT](./LICENSE)
