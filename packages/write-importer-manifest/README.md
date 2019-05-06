# @pnpm/write-importer-manifest

> Write an importer manifest (called package.json in most cases)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/write-importer-manifest.svg)](https://www.npmjs.com/package/@pnpm/write-importer-manifest)
<!--/@-->

## Installation

```sh
<npm|yarn|pnpm> add @pnpm/write-importer-manifest
```

## Usage

```ts
import writeImporterManifest from '@pnpm/write-importer-manifest'
import path = require('path')

(async () => await writeImporterManifest(path.resolve('package.yaml'), { name: 'foo', version: '1.0.0' }))()
```

## License

MIT © [Zoltan Kochan](https://www.kochan.io/)
