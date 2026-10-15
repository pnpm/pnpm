# @pnpm/exec.lifecycle

> Package lifecycle hook runner

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/exec.lifecycle.svg)](https://npmx.dev/package/@pnpm/exec.lifecycle)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/logger @pnpm/exec.lifecycle
```

## Usage

```ts
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { runLifecycleHook, runPostinstallHooks } from '@pnpm/exec.lifecycle'

const targetPkgRoot = path.resolve('node_modules/target-pkg')
const pkg = JSON.parse(await readFile(path.join(targetPkgRoot, 'package.json'), 'utf8'))

// Run a specific hook
await runLifecycleHook('preinstall', pkg, {
  depPath: 'target-pkg@1.0.0',
  pkgRoot: targetPkgRoot,
  rootModulesDir: path.resolve('node_modules'),
  unsafePerm: true,
})

// Run all install hooks
await runPostinstallHooks({
  depPath: 'target-pkg@1.0.0',
  pkgRoot: targetPkgRoot,
  rootModulesDir: path.resolve('node_modules'),
  unsafePerm: true,
})
```

## API

### `runLifecycleHook(stage, packageManifest, opts): Promise<boolean>`

### `runPostinstallHooks(opts): Promise<boolean>`

## License

MIT
