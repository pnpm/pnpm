# @pnpm/read-project-manifest

> Read a project manifest (called package.json in most cases)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/read-project-manifest.svg)](https://www.npmjs.com/package/@pnpm/read-project-manifest)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/read-project-manifest
```

## Usage

```ts
import { readProjectManifest } from '@pnpm/read-project-manifest'

const { manifest, fileName } = await readProjectManifest(process.cwd())
```

## License

MIT
