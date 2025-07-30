import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from '../utils'

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
      url: 'https://github.com/denoland/deno/releases/download/v2.4.2/deno-aarch64-apple-darwin.zip',
      integrity: 'sha256-cy885Q3GSmOXLKTvtIZ5KZwBZjzpGPcQ1pWmjOX0yTY=',
      bin: 'deno',
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
      url: 'https://github.com/denoland/deno/releases/download/v2.4.2/deno-aarch64-unknown-linux-gnu.zip',
      integrity: 'sha256-SjIY48qZ8qu8QdIGkbynlC0Y68sB22tDicu5HqvxBV8=',
      bin: 'deno',
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
      url: 'https://github.com/denoland/deno/releases/download/v2.4.2/deno-x86_64-apple-darwin.zip',
      integrity: 'sha256-+kfrcrjR80maf7Pmx7vNOx5kBxErsD+v1AqoA4pUuT4=',
      bin: 'deno',
    },
  },
  {
    targets: [
      {
        os: 'win32',
        cpu: 'x64',
      },
      {
        os: 'win32',
        cpu: 'arm64',
      },
    ],
    resolution: {
      type: 'binary',
      archive: 'zip',
      url: 'https://github.com/denoland/deno/releases/download/v2.4.2/deno-x86_64-pc-windows-msvc.zip',
      integrity: 'sha256-WoyBb25yA3inTCVnZ5uip5nIFbjC/8BrDnHabCqb8Yk=',
      bin: 'deno.exe',
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
      url: 'https://github.com/denoland/deno/releases/download/v2.4.2/deno-x86_64-unknown-linux-gnu.zip',
      integrity: 'sha256-2Ed4YzIVt8uTz3aQhg1iQfYysIe9KhneEs1BDmsuFXo=',
      bin: 'deno',
    },
  },
]

test('installing Deno runtime', async () => {
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['deno@runtime:2.4.2'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/deno')
  expect(project.readLockfile()).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          deno: {
            specifier: 'runtime:2.4.2',
            version: 'runtime:2.4.2',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'deno@runtime:2.4.2': {
        hasBin: true,
        resolution: {
          type: 'variations',
          variants: RESOLUTIONS,
        },
        version: '2.4.2',
      },
    },
    snapshots: {
      'deno@runtime:2.4.2': {},
    },
  })

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }, {
    offline: true, // We want to verify that Deno is resolved from cache.
  }))
  project.isExecutable('.bin/deno')

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
          deno: {
            specifier: 'runtime:2.4.2',
            version: 'runtime:2.4.2',
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
      'deno@runtime:2.4.2': {
        hasBin: true,
        resolution: {
          type: 'variations',
          variants: RESOLUTIONS,
        },
        version: '2.4.2',
      },
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
    },
    snapshots: {
      'deno@runtime:2.4.2': {},
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {},
    },
  })
})

test('installing Deno runtime fails if offline mode is used and Deno not found locally', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage({}, ['deno@runtime:2.4.2'], testDefaults({ fastUnpack: false }, { offline: true }))
  ).rejects.toThrow(/Failed to resolve deno@2.4.2 in package mirror/)
})

test('installing Deno runtime fails if integrity check fails', async () => {
  prepareEmpty()

  writeYamlFile(WANTED_LOCKFILE, {
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        devDependencies: {
          deno: {
            specifier: 'runtime:2.4.2',
            version: 'runtime:2.4.2',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'deno@runtime:2.4.2': {
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
        version: '2.4.2',
      },
    },
    snapshots: {
      'deno@runtime:2.4.2': {},
    },
  }, {
    lineWidth: -1,
  })

  const manifest = {
    devDependencies: {
      deno: 'runtime:2.4.2',
    },
  }
  await expect(install(manifest, testDefaults({ frozenLockfile: true }, {
    retry: {
      retries: 0,
    },
  }))).rejects.toThrow(/Got unexpected checksum for/)
})
