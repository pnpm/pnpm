import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

import { iteratePkgsForVirtualStore } from '../src/iteratePkgsForVirtualStore.js'

// A dependency's own `engines.runtime` pin adds a `node@runtime:` entry that
// sorts before the root project's pin. The engine part of a built package's
// global virtual store path must still follow the root project's pin.
test('a built package is keyed by the root project runtime, not a dependency runtime', () => {
  const builtDir = (rootNodeVersion: string) => globalVirtualStoreDirOf(lockfileWithRootRuntime(rootNodeVersion), 'built@1.0.0')

  expect(builtDir('24.0.0')).not.toBe(builtDir('22.0.0'))
})

function lockfileWithRootRuntime (rootNodeVersion: string): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { built: '1.0.0', pinning: '1.0.0', node: `runtime:${rootNodeVersion}` },
        dependencies: { built: '1.0.0', pinning: '1.0.0' },
        devDependencies: { node: `runtime:${rootNodeVersion}` },
      },
    },
    packages: {
      ['built@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-built' } },
      ['node@runtime:22.0.0' as DepPath]: { resolution: { integrity: 'sha512-node22' } },
      ['node@runtime:24.0.0' as DepPath]: { resolution: { integrity: 'sha512-node24' } },
      ['pinning@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-pinning' },
        dependencies: { node: 'runtime:22.0.0' },
      },
    },
  }
}

function globalVirtualStoreDirOf (lockfile: LockfileObject, depPath: string): string | undefined {
  for (const { pkgMeta, dirInVirtualStore } of iteratePkgsForVirtualStore(lockfile, {
    allowBuild: (candidate) => candidate === 'built@1.0.0',
    enableGlobalVirtualStore: true,
    globalVirtualStoreDir: '/gvs',
    lockfileDir: '/project',
    virtualStoreDir: '/project/node_modules/.pnpm',
    virtualStoreDirMaxLength: 120,
  })) {
    if (pkgMeta.depPath === depPath) return dirInVirtualStore
  }
  return undefined
}
