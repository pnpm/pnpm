# @pnpm/lifecycle

> Package lifecycle hook runner

<!--@shields('npm', 'travis')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/lifecycle.svg)](https://www.npmjs.com/package/@pnpm/lifecycle) [![Build Status](https://img.shields.io/travis/pnpm/lifecycle/master.svg)](https://travis-ci.org/pnpm/lifecycle)
<!--/@-->

## Installation

```sh
npm i -S @pnpm/logger @pnpm/lifecycle
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
  rawNpmConfig: {},
  rootNodeModulesDir: path.resolve('node_modules'),
  unsafePerm: true,
})

// Run all install hooks
await runPostinstallHooks({
  pkgId: 'target-pkg/1.0.0',
  pkgRoot: targetPkgRoot,
  rawNpmConfig: {},
  rootNodeModulesDir: path.resolve('node_modules'),
  unsafePerm: true,
})
```

## API

### `runLifecycleHook(stage, packageJson, opts): Promise<void>`

### `runPostinstallHooks(opts): Promise<void>`

## License

[MIT](./LICENSE) © [Zoltan Kochan](https://www.kochan.io/)
