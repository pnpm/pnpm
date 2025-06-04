# @pnpm/link-bins

> Link bins to node_modules/.bin

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/link-bins.svg)](https://www.npmjs.com/package/@pnpm/link-bins)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/link-bins
```

## Usage

```ts
import linkBins, {linkBinsOfPackages} from '@pnpm/link-bins'

function warn (msg) { console.warn(msg) }

await linkBins('node_modules', 'node_modules/.bin', {warn})

const packages = [{manifest: packageJson, location: pathToPackage}]
await linkBinsOfPackages(packages, 'node_modules/.bin', {warn})
```

## License

[MIT](./LICENSE)
