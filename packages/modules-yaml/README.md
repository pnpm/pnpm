# @pnpm/modules-yaml

> Reads/writes \`node_modules/.modules.yaml\`

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/modules-yaml.svg)](https://www.npmjs.com/package/@pnpm/modules-yaml)
<!--/@-->

## Installation

```sh
<pnpm|npm|yarn> add @pnpm/modules-yaml
```

## Usage

```ts
import {write, read} from '@pnpm/modules-yaml'

await write('node_modules', {
  hoistedAliases: {}
  layoutVersion: 1,
  packageManager: 'pnpm@1.0.0',
  pendingBuilds: [],
  shamefullyFlatten: false,
  skipped: [],
  storeDir: '/home/user/.pnpm-store',
})

const modulesYaml = await read(`node_modules`)
```

## API

### `read(pathToDir): Promise<ModulesObject>`

Reads `.modules.yaml` from the specified directory.

### `write(pathToDir, ModulesObject): Promise<void>`

Writes a `.modules.yaml` file to the specified directory.

## License

MIT © [Zoltan Kochan](https://www.kochan.io/)
