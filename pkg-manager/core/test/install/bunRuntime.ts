import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from '../utils/index.js'

const RESOLUTIONS = [
  {
    targets: [
      {
        os: 'darwin',
        cpu: 'arm64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-darwin-aarch64.zip',
      integrity: 'sha256-Z0pIN4NC76rcPCkVlrVzAQ88I4iVj3xEZ42H9vt1mZE=',
      prefix: 'bun-darwin-aarch64',
      bin: 'bun',
    },
  },
  {
    targets: [
      {
        os: 'darwin',
        cpu: 'x64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-darwin-x64.zip',
      integrity: 'sha256-39fkxHMRtdvTgjCzz9NX9dC+ro75eZYsW0EAj8QcJaA=',
      prefix: 'bun-darwin-x64',
      bin: 'bun',
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 'arm64',
        libc: 'musl',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-linux-aarch64-musl.zip',
      integrity: 'sha256-ECBLT4ZeQCUI1pVr75O+Y11qek3cl0lCGxY2qseZZbY=',
      prefix: 'bun-linux-aarch64-musl',
      bin: 'bun',
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 'arm64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-linux-aarch64.zip',
      integrity: 'sha256-/P1HHNvVp4/Uo5DinMzSu3AEpJ01K6A3rzth1P1dC4M=',
      prefix: 'bun-linux-aarch64',
      bin: 'bun',
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 'x64',
        libc: 'musl',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-linux-x64-musl.zip',
      integrity: 'sha256-3M13Zi0KtkLSgO704yFtYCru4VGfdTXKHYOsqRjo/os=',
      prefix: 'bun-linux-x64-musl',
      bin: 'bun',
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 'x64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-linux-x64.zip',
      integrity: 'sha256-w9PBTppeyD/2fQrP525DFa0G2p809Z/HsTgTeCyvH2Y=',
      prefix: 'bun-linux-x64',
      bin: 'bun',
    },
  },
  {
    targets: [
      {
        os: 'win32',
        cpu: 'x64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.2.19/bun-windows-x64.zip',
      integrity: 'sha256-pIj0ZM5nsw4Ayw6lay9i5JuBw/zqe6kkYdNiJLBvdfg=',
      prefix: 'bun-windows-x64',
      bin: 'bun.exe',
    },
  },
]

test('installing Bun runtime', async () => {
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['bun@runtime:1.2.19'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/bun')
  expect(project.readLockfile()).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          bun: {
            specifier: 'runtime:1.2.19',
            version: 'runtime:1.2.19',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'bun@runtime:1.2.19': {
        hasBin: true,
        resolution: {
          type: 'variations',
          variants: RESOLUTIONS,
        },
        version: '1.2.19',
      },
    },
    snapshots: {
      'bun@runtime:1.2.19': {},
    },
  })

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }, {
    offline: true, // We want to verify that Bun is resolved from cache.
  }))
  project.isExecutable('.bin/bun')

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
          bun: {
            specifier: 'runtime:1.2.19',
            version: 'runtime:1.2.19',
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
      'bun@runtime:1.2.19': {
        hasBin: true,
        resolution: {
          type: 'variations',
          variants: RESOLUTIONS,
        },
        version: '1.2.19',
      },
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
    },
    snapshots: {
      'bun@runtime:1.2.19': {},
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {},
    },
  })
})

test('installing Bun runtime fails if offline mode is used and Bun not found locally', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage({}, ['bun@runtime:1.2.19'], testDefaults({ fastUnpack: false }, { offline: true }))
  ).rejects.toThrow(/Failed to resolve bun@1.2.19 in package mirror/)
})

test('installing Bun runtime fails if integrity check fails', async () => {
  prepareEmpty()

  writeYamlFile(WANTED_LOCKFILE, {
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        devDependencies: {
          bun: {
            specifier: 'runtime:1.2.19',
            version: 'runtime:1.2.19',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'bun@runtime:1.2.19': {
        hasBin: true,
        resolution: {
          type: 'variations',
          variants: RESOLUTIONS.map((resolutionVariant) => ({
            ...resolutionVariant,
            resolution: {
              ...resolutionVariant.resolution,
              integrity: 'sha256-0000000000000000000000000000000000000000000=',
            },
          })),
        },
        version: '1.2.19',
      },
    },
    snapshots: {
      'bun@runtime:1.2.19': {},
    },
  }, {
    lineWidth: -1,
  })

  const manifest = {
    devDependencies: {
      bun: 'runtime:1.2.19',
    },
  }
  await expect(install(manifest, testDefaults({ frozenLockfile: true }, {
    retry: {
      retries: 0,
    },
  }))).rejects.toThrow(/Got unexpected checksum for/)
})
