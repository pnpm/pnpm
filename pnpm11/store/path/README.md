# @pnpm/store.path

> Resolves the pnpm store path

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/store.path.svg)](https://npmx.dev/package/@pnpm/store.path)
<!--/@-->

## Installation

```sh
<pnpm|yarn|npm> add @pnpm/store.path
```

## Usage

```ts
import { getStorePath } from '@pnpm/store.path'

await getStorePath({ pkgRoot: 'F:\\project', storePath: 'pnpm-store', pnpmHomeDir: 'C:\\pnpm-home' })
//> F:\project\pnpm-store\v11
```

## License

[MIT](./LICENSE)
