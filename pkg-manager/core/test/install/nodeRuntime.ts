import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from '../utils'

const NODE_INTEGRITIES = {
  'aix-ppc64': 'sha256-13Q/3fXoZxJPVVqR9scpEE/Vx12TgvEChsP7s/0S7wc=',
  'darwin-arm64': 'sha256-6pbTSc+qZ6qHzuqj5bUskWf3rDAv2NH/Fi0HhencB4U=',
  'darwin-x64': 'sha256-Qio4h/9UGPCkVS2Jz5k0arirUbtdOEZguqiLhETSwRE=',
  'linux-arm64': 'sha256-HTVHImvn5ZrO7lx9Aan4/BjeZ+AVxaFdjPOFtuAtBis=',
  'linux-armv7l': 'sha256-0h239Xxc4YKuwrmoPjKVq8N+FzGrtzmV09Vz4EQJl3w=',
  'linux-ppc64le': 'sha256-OwmNzPVtRGu7gIRdNbvsvbdGEoYNFpDzohY4fJnJ1iA=',
  'linux-s390x': 'sha256-fsX9rQyBnuoXkA60PB3pSNYgp4OxrJQGLKpDh3ipKzA=',
  'linux-x64': 'sha256-dLsPOoAwfFKUIcPthFF7j1Q4Z3CfQeU81z35nmRCr00=',
  'win32-arm64': 'sha256-N2Ehz0a9PAJcXmetrhkK/14l0zoLWPvA2GUtczULOPA=',
  'win32-x64': 'sha256-MtY5tH1MCmUf+PjX1BpFQWij1ARb43mF+agQz4zvYXQ=',
  'win32-x86': 'sha256-4BNPUBcVSjN2csf7zRVOKyx3S0MQkRhWAZINY9DEt9A=',
}

test('installing Node.js runtime', async () => {
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['node@runtime:22.0.0'], testDefaults({ fastUnpack: false }))

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
            specifier: 'runtime:22.0.0',
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
          integrities: NODE_INTEGRITIES,
          type: 'nodeRuntime',
        },
      },
    },
    snapshots: {
      'node@runtime:22.0.0': {},
    },
  })

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }, {
    offline: true, // We want to verify that Node.js is resolved from cache.
  }))
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
            specifier: 'runtime:22.0.0',
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
          integrities: NODE_INTEGRITIES,
          type: 'nodeRuntime',
        },
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

test('installing node.js runtime fails if offline mode is used and node.js not found locally', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage({}, ['node@runtime:22.0.0'], testDefaults({ fastUnpack: false }, { offline: true }))
  ).rejects.toThrow(/Offline Node.js resolution is not supported/)
})

test('installing Node.js runtime from RC channel', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['node@runtime:24.0.0-rc.4'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/node')
})

test('installing Node.js runtime fails if integrity check fails', async () => {
  prepareEmpty()

  writeYamlFile(WANTED_LOCKFILE, {
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        devDependencies: {
          node: {
            specifier: 'runtime:22.0.0',
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
          integrities: {
            ...NODE_INTEGRITIES,
            [`${process.platform}-${process.arch}`]: 'sha256-0000000000000000000000000000000000000000000=',
          },
          type: 'nodeRuntime',
        },
      },
    },
    snapshots: {
      'node@runtime:22.0.0': {},
    },
  })

  const manifest = {
    devDependencies: {
      node: 'runtime:22.0.0',
    },
  }
  await expect(install(manifest, testDefaults({ frozenLockfile: true }, {
    retry: {
      retries: 0,
    },
  }))).rejects.toThrow(/Got unexpected checksum for/)
})
