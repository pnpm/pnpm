# @pnpm/registry.pkg-doc-filter

> Filters the package document from the registry

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/registry.pkg-doc-filter.svg)](https://www.npmjs.com/package/@pnpm/registry.pkg-doc-filter)
<!--/@-->

## Installation

```
pnpm add @pnpm/registry.pkg-doc-filter
```

## Usage

```ts
import { filterPkgDocByPublishDate } from '@pnpm/registry.pkg-doc-filter'

const pkgDoc = await (await fetch('https://registry.npmjs.org/is-odd')).json()

// Keep only those versions in the document that were published before Jan 1 2023.
filterPkgDocByPublishDate(pkgDoc, new Date('2023-01-01'))
```

## License

MIT
