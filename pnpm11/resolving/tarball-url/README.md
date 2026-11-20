# @pnpm/resolving.tarball-url

> Build and recognize the canonical tarball URL of an npm package

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/resolving.tarball-url.svg)](https://npmx.dev/package/@pnpm/resolving.tarball-url)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/resolving.tarball-url
```

## Usage

```ts
import {
  getNpmTarballUrl,
  isCanonicalRegistryTarballUrl,
} from '@pnpm/resolving.tarball-url'

const registry = 'https://registry.npmjs.org/'

getNpmTarballUrl('foo', '1.0.0', { registry })
//=> 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz'

isCanonicalRegistryTarballUrl(
  'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  { name: 'foo', version: '1.0.0' },
  registry
)
//=> true
```

## License

MIT
