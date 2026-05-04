# @pnpm/bins.linker

> Link bins to node_modules/.bin

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/bins.linker.svg)](https://npmx.dev/package/@pnpm/bins.linker)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/bins.linker
```

## Usage

```ts
import linkBins, {linkBinsOfPackages} from '@pnpm/bins.linker'

function warn (msg) { console.warn(msg) }

await linkBins('node_modules', 'node_modules/.bin', {warn})

const packages = [{manifest: packageJson, location: pathToPackage}]
await linkBinsOfPackages(packages, 'node_modules/.bin', {warn})
```

## License

[MIT](./LICENSE)
