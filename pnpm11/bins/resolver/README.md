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
import { readFile } from 'node:fs/promises'
import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'

const manifest = JSON.parse(await readFile('package.json', 'utf8'))
await getBinsFromPackageManifest(manifest, process.cwd())
//> [{name: 'bin-name', path: 'path-to-bin'}]
```

## API

### `getBinsFromPackageManifest(manifest, pkgPath): Promise<Array<{name: string, path: string}>>`

## License

[MIT](./LICENSE)
