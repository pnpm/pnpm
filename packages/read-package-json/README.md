# @pnpm/read-package-json

> Read a package.json

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/read-package-json.svg)](https://www.npmjs.com/package/@pnpm/read-package-json)
<!--/@-->

## Installation

```sh
<pnpm|npm|yarn> add @pnpm/read-package-json
```

## Usage

```ts
import readPackageJson from '@pnpm/read-package-json'

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
