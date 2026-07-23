import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

import { getGlobalVirtualStoreHoistProjection } from '../src/getGlobalVirtualStoreHoistProjection.js'
import { iteratePkgsForVirtualStore } from '../src/iteratePkgsForVirtualStore.js'

const include = {
  dependencies: true,
  devDependencies: true,
  optionalDependencies: true,
}
const rootImporter = ['.' as ProjectId]

test('projects resolver-visible root and hoisted dependencies', () => {
  const lockfile = {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: {
          consumer: 'consumer@1.0.0',
          runtime: 'runtime@1.0.0',
        },
      },
    },
    packages: {
      'consumer@1.0.0': {
        dependencies: {
          transitive: 'transitive@1.0.0',
        },
        resolution: { integrity: 'sha512-consumer' },
      },
      'runtime@1.0.0': {
        resolution: { integrity: 'sha512-runtime' },
      },
      'transitive@1.0.0': {
        resolution: { integrity: 'sha512-transitive' },
      },
    },
  } as unknown as LockfileObject

  expect(getGlobalVirtualStoreHoistProjection(lockfile, {
    hoistPattern: ['*'],
    importerIds: rootImporter,
    include,
    skipped: new Set(),
  })).toEqual({
    consumer: 'consumer@1.0.0',
    runtime: 'runtime@1.0.0',
    transitive: 'transitive@1.0.0',
  })
})

test('excludes dependency groups omitted from the install', () => {
  const lockfile = {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: {
          consumer: 'consumer@1.0.0',
        },
        devDependencies: {
          tooling: 'tooling@1.0.0',
        },
      },
    },
    packages: {
      'consumer@1.0.0': {
        resolution: { integrity: 'sha512-consumer' },
      },
      'tooling@1.0.0': {
        resolution: { integrity: 'sha512-tooling' },
      },
    },
  } as unknown as LockfileObject

  expect(getGlobalVirtualStoreHoistProjection(lockfile, {
    importerIds: rootImporter,
    include: { ...include, devDependencies: false },
    skipped: new Set(),
  })).toEqual({
    consumer: 'consumer@1.0.0',
  })
})

test('excludes checkout-local directory dependencies', () => {
  const lockfile = {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: {
          consumer: 'consumer@1.0.0',
          local: 'file:../local',
        },
      },
    },
    packages: {
      'consumer@1.0.0': {
        resolution: { integrity: 'sha512-consumer' },
      },
      'local@file:../local': {
        resolution: { directory: '../local', type: 'directory' },
      },
    },
  } as unknown as LockfileObject

  expect(getGlobalVirtualStoreHoistProjection(lockfile, {
    importerIds: rootImporter,
    include,
    skipped: new Set(),
  })).toEqual({
    consumer: 'consumer@1.0.0',
  })
})

test('resolver-visible dependencies are included in global virtual store hashes', () => {
  const getConsumerDir = (runtimeVersion: string): string => {
    const runtimeDepPath = `runtime@${runtimeVersion}` as DepPath
    const lockfile = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            consumer: 'consumer@1.0.0',
            runtime: runtimeDepPath,
          },
        },
      },
      packages: {
        'consumer@1.0.0': {
          resolution: { integrity: 'sha512-consumer' },
        },
        [runtimeDepPath]: {
          resolution: { integrity: `sha512-runtime-${runtimeVersion}` },
        },
      },
    } as unknown as LockfileObject
    const consumer = Array.from(iteratePkgsForVirtualStore(lockfile, {
      enableGlobalVirtualStore: true,
      globalVirtualStoreDir: '/store/links',
      importerIds: rootImporter,
      include,
      skipped: new Set(),
      virtualStoreDir: '/project/node_modules/.pnpm',
      virtualStoreDirMaxLength: 120,
    })).find(({ pkgMeta }) => pkgMeta.depPath === 'consumer@1.0.0')
    return consumer!.dirInVirtualStore
  }

  expect(getConsumerDir('1.0.0')).not.toBe(getConsumerDir('2.0.0'))
})
