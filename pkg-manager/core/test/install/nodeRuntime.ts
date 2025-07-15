import { LOCKFILE_VERSION } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import { testDefaults } from '../utils'

test('installing node.js runtime', async () => {
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['node@runtime:node@22.0.0'], testDefaults({ fastUnpack: false }))

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
})
