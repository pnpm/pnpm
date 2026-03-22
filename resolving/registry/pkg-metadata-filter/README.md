# @pnpm/registry.pkg-metadata-filter

> Filters the package metadata from the registry

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/registry.pkg-metadata-filter.svg)](https://www.npmjs.com/package/@pnpm/registry.pkg-metadata-filter)
<!--/@-->

## Installation

```
pnpm add @pnpm/registry.pkg-metadata-filter
```

## Usage

```ts
import { filterPkgMetadataByPublishDate } from '@pnpm/registry.pkg-metadata-filter'

const pkgDoc = await (await fetch('https://registry.npmjs.org/is-odd')).json()

// Keep only those versions in the document that were published before Jan 1 2023.
filterPkgMetadataByPublishDate(pkgDoc, new Date('2023-01-01'))
```

## License

MIT
