# @pnpm/write-project-manifest

> Write a project manifest (called package.json in most cases)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/write-project-manifest.svg)](https://www.npmjs.com/package/@pnpm/write-project-manifest)
<!--/@-->

## Installation

```sh
<npm|yarn|pnpm> add @pnpm/write-project-manifest
```

## Usage

```ts
import writeProjectManifest from '@pnpm/write-project-manifest'
import path from 'path'

(async () => await writeProjectManifest(path.resolve('package.yaml'), { name: 'foo', version: '1.0.0' }))()
```

## License

MIT
