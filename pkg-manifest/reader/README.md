# @pnpm/pkg-manifest.reader

> Read a package.json

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/pkg-manifest.reader.svg)](https://www.npmjs.com/package/@pnpm/pkg-manifest.reader)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/pkg-manifest.reader
```

## Usage

```ts
import { readPackageJson } from '@pnpm/pkg-manifest.reader'

const pkgJson = await readPackageJson('package.json')
```

## API

### default: `readPackageJson(path): Promise<PackageManifest>`

### Arguments

- `path` - _String_ - path to the `package.json`

### `fromDir(path): Promise<PackageManifest>`

### Arguments

- `path` - _String_ - path to the directory with the `package.json`

## License

MIT
