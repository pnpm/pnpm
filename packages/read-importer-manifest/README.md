# @pnpm/read-importer-manifest

> Read an importer manifest (called package.json in most cases)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/read-importer-manifest.svg)](https://www.npmjs.com/package/@pnpm/read-importer-manifest)
<!--/@-->

## Installation

```sh
<npm|yarn|pnpm> add @pnpm/read-importer-manifest
```

## Usage

```ts
import readImporterManifest from '@pnpm/read-importer-manifest'

const { manifest, fileName } = await readImporterManifest(process.cwd())
```

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
