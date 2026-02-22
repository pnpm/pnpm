import fs from 'fs'
import path from 'path'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { type VariationsResolution } from '@pnpm/resolver-base'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from '../utils/index.js'

// The standard glibc variants from nodejs.org/download/release/
const GLIBC_RESOLUTIONS = [
  {
    targets: [
      {
        os: 'aix',
        cpu: 'ppc64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-aix-ppc64.tar.gz',
      integrity: 'sha256-13Q/3fXoZxJPVVqR9scpEE/Vx12TgvEChsP7s/0S7wc=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
    },
  },
  {
    targets: [
      {
        os: 'darwin',
        cpu: 'arm64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz',
      integrity: 'sha256-6pbTSc+qZ6qHzuqj5bUskWf3rDAv2NH/Fi0HhencB4U=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
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
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-darwin-x64.tar.gz',
      integrity: 'sha256-Qio4h/9UGPCkVS2Jz5k0arirUbtdOEZguqiLhETSwRE=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
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
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-linux-arm64.tar.gz',
      integrity: 'sha256-HTVHImvn5ZrO7lx9Aan4/BjeZ+AVxaFdjPOFtuAtBis=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 'armv7l',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-linux-armv7l.tar.gz',
      integrity: 'sha256-0h239Xxc4YKuwrmoPjKVq8N+FzGrtzmV09Vz4EQJl3w=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 'ppc64le',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-linux-ppc64le.tar.gz',
      integrity: 'sha256-OwmNzPVtRGu7gIRdNbvsvbdGEoYNFpDzohY4fJnJ1iA=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
    },
  },
  {
    targets: [
      {
        os: 'linux',
        cpu: 's390x',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-linux-s390x.tar.gz',
      integrity: 'sha256-fsX9rQyBnuoXkA60PB3pSNYgp4OxrJQGLKpDh3ipKzA=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
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
      archive: 'tarball',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-linux-x64.tar.gz',
      integrity: 'sha256-dLsPOoAwfFKUIcPthFF7j1Q4Z3CfQeU81z35nmRCr00=',
      bin: {
        node: 'bin/node',
        npm: 'lib/node_modules/npm/bin/npm-cli.js',
        npx: 'lib/node_modules/npm/bin/npx-cli.js',
      },
    },
  },
  {
    targets: [
      {
        os: 'win32',
        cpu: 'arm64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-win-arm64.zip',
      integrity: 'sha256-N2Ehz0a9PAJcXmetrhkK/14l0zoLWPvA2GUtczULOPA=',
      bin: {
        node: 'node.exe',
        npm: 'node_modules/npm/bin/npm-cli.js',
        npx: 'node_modules/npm/bin/npx-cli.js',
      },
      prefix: 'node-v22.0.0-win-arm64',
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
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-win-x64.zip',
      integrity: 'sha256-MtY5tH1MCmUf+PjX1BpFQWij1ARb43mF+agQz4zvYXQ=',
      bin: {
        node: 'node.exe',
        npm: 'node_modules/npm/bin/npm-cli.js',
        npx: 'node_modules/npm/bin/npx-cli.js',
      },
      prefix: 'node-v22.0.0-win-x64',
    },
  },
  {
    targets: [
      {
        os: 'win32',
        cpu: 'x86',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://nodejs.org/download/release/v22.0.0/node-v22.0.0-win-x86.zip',
      integrity: 'sha256-4BNPUBcVSjN2csf7zRVOKyx3S0MQkRhWAZINY9DEt9A=',
      bin: {
        node: 'node.exe',
        npm: 'node_modules/npm/bin/npm-cli.js',
        npx: 'node_modules/npm/bin/npx-cli.js',
      },
      prefix: 'node-v22.0.0-win-x86',
    },
  },
]

test('installing Node.js runtime', async () => {
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['node@runtime:22.0.0'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/node')
  expect(fs.readlinkSync('node_modules/node')).toContain(path.join('links', '@', 'node', '22.0.0'))

  const lockfile = project.readLockfile()
  expect(lockfile).toStrictEqual({
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
          type: 'variations',
          // Musl variants from unofficial-builds.nodejs.org are appended alongside
          // the standard glibc variants, so use arrayContaining to allow them.
          variants: expect.arrayContaining(GLIBC_RESOLUTIONS),
        },
        version: '22.0.0',
      },
    },
    snapshots: {
      'node@runtime:22.0.0': {},
    },
  })
  // Verify that musl variants are present for linux x64 and arm64.
  const variants = (lockfile.packages['node@runtime:22.0.0'].resolution as VariationsResolution).variants
  expect(variants).toContainEqual(expect.objectContaining({
    targets: [{ os: 'linux', cpu: 'x64', libc: 'musl' }],
    resolution: expect.objectContaining({
      url: expect.stringContaining('unofficial-builds.nodejs.org'),
    }),
  }))

  // Verify that package.json is created
  expect(fs.existsSync(path.resolve('node_modules/node/package.json'))).toBeTruthy()

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }, {
    offline: true, // We want to verify that Node.js is resolved from cache.
  }))
  project.isExecutable('.bin/node')
  expect(fs.readlinkSync('node_modules/node')).toContain(path.join('links', '@', 'node', '22.0.0'))

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
          type: 'variations',
          variants: expect.arrayContaining(GLIBC_RESOLUTIONS),
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
          type: 'variations',
          variants: GLIBC_RESOLUTIONS.map((resolutionVariant) => ({
            ...resolutionVariant,
            resolution: {
              ...resolutionVariant.resolution,
              integrity: 'sha256-0000000000000000000000000000000000000000000=',
            },
          })),
        },
        version: '22.0.0',
      },
    },
    snapshots: {
      'node@runtime:22.0.0': {},
    },
  }, {
    lineWidth: -1,
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

test('installing Node.js runtime for the given supported architecture', async () => {
  const isWindows = process.platform === 'win32'
  const supportedArchitectures = {
    os: [isWindows ? 'linux' : 'win32'],
    cpu: ['x64'],
  }
  const expectedBinLocation = isWindows ? 'node/bin/node' : 'node/node.exe'
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['node@runtime:22.0.0'],
    testDefaults({
      fastUnpack: false,
      supportedArchitectures,
    })
  )
  project.has(expectedBinLocation)
  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true, supportedArchitectures }))
  project.has(expectedBinLocation)
})

test('installing Node.js runtime, when it is set via the engines field of a dependency', async () => {
  prepareEmpty()
  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/cli-with-node-engine@1.0.0'],
    testDefaults({
      fastUnpack: false,
      allowBuilds: { '@pnpm.e2e/cli-with-node-engine': true },
      neverBuiltDependencies: undefined,
    })
  )
  expect(fs.readFileSync('node_modules/@pnpm.e2e/cli-with-node-engine/node-version', 'utf8')).toBe('v22.19.0')
})
