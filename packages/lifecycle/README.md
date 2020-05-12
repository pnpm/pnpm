# @pnpm/lifecycle

> Package lifecycle hook runner

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/lifecycle.svg)](https://www.npmjs.com/package/@pnpm/lifecycle)
<!--/@-->

## Installation

```sh
<pnpm|npm|yarn> add @pnpm/logger @pnpm/lifecycle
```

## Usage

```ts
import runLifecycleHook, {runPostinstallHooks} from '@pnpm/lifecycle'

const targetPkgRoot = path.resolve('node_modules/target-pkg')
const pkg = require(path.join(targetPkgRoot, 'package.json'))

// Run a specific hook
await runLifecycleHook('preinstall', pkg, {
  pkgId: 'target-pkg/1.0.0',
  pkgRoot: targetPkgRoot,
  rawConfig: {},
  rootModulesDir: path.resolve('node_modules'),
  unsafePerm: true,
})

// Run all install hooks
await runPostinstallHooks({
  pkgId: 'target-pkg/1.0.0',
  pkgRoot: targetPkgRoot,
  rawConfig: {},
  rootModulesDir: path.resolve('node_modules'),
  unsafePerm: true,
})
```

## API

### `runLifecycleHook(stage, packageManifest, opts): Promise<void>`

### `runPostinstallHooks(opts): Promise<void>`

## License

MIT © [Zoltan Kochan](https://www.kochan.io/)
