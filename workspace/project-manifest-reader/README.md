# @pnpm/workspace.project-manifest-reader

> Read a project manifest (called package.json in most cases)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/workspace.project-manifest-reader.svg)](https://npmx.dev/package/@pnpm/workspace.project-manifest-reader)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/workspace.project-manifest-reader
```

## Usage

```ts
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'

const { manifest, fileName } = await readProjectManifest(process.cwd())
```

## License

MIT
