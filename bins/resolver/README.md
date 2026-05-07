# @pnpm/bins.resolver

> Returns bins of a package

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/bins.resolver.svg)](https://npmx.dev/package/@pnpm/bins.resolver)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/bins.resolver
```

## Usage

```ts
import getBinsFromPkg from '@pnpm/bins.resolver'

await getBinsFromPkg(path.resolve('package.json'), process.cwd())
//> [{name: 'bin-name', path: 'path-to-bin'}]
```

## API

### `getBinsFromPkg(packageJson, pathToPkg): Promise<Array<{name: string, path: string}>>`

## License

[MIT](./LICENSE)
