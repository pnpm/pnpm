# @pnpm/workspace.project-manifest-writer

> Write a project manifest (called package.json in most cases)

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/workspace.project-manifest-writer.svg)](https://www.npmjs.com/package/@pnpm/workspace.project-manifest-writer)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/workspace.project-manifest-writer
```

## Usage

```ts
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import path from 'path'

(async () => await writeProjectManifest(path.resolve('package.yaml'), { name: 'foo', version: '1.0.0' }))()
```

## License

MIT
