import { LOCKFILE_VERSION } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { testDefaults } from '../utils'

test('installing node.js runtime', async () => {
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['node@runtime:node@22.0.0'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/node')
  expect(project.readLockfile()).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          node: {
            specifier: 'runtime:node@22.0.0',
            version: 'runtime:22.0.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'node@runtime:22.0.0': {
        hasBin: true,
        resolution: {
          integrity: 'sha256-NexAQ7DxOFuPb9J7KNeuLtuSeaxFVUGlTrqSqs7AEbo=',
          type: 'nodeRuntime',
        },
        version: '22.0.0',
      },
    },
    snapshots: {
      'node@runtime:22.0.0': {},
    },
  })

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }))
  project.isExecutable('.bin/node')

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'], testDefaults({ fastUnpack: false }))
  project.has('@pnpm.e2e/dep-of-pkg-with-1-dep')

  expect(project.readLockfile()).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          node: {
            specifier: 'runtime:node@22.0.0',
            version: 'runtime:22.0.0',
          },
          '@pnpm.e2e/dep-of-pkg-with-1-dep': {
            specifier: '100.1.0',
            version: '100.1.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'node@runtime:22.0.0': {
        hasBin: true,
        resolution: {
          integrity: 'sha256-NexAQ7DxOFuPb9J7KNeuLtuSeaxFVUGlTrqSqs7AEbo=',
          type: 'nodeRuntime',
        },
        version: '22.0.0',
      },
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
    },
    snapshots: {
      'node@runtime:22.0.0': {},
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {},
    },
  })
})
