import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, describe, expect, jest, test } from '@jest/globals'
import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId, ProjectManifest, RegistriesByScope } from '@pnpm/types'

import type { GetPackageInfoOptions, PackageInfo } from '../lib/getPkgInfo.js'
import type { LicensePackage } from '../lib/licenses.js'

const tmpStoreDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-license-spec-'))
afterAll(() => {
  fs.rmSync(tmpStoreDir, { recursive: true, force: true })
})

const actualModule = await import('../lib/getPkgInfo.js')
jest.unstable_mockModule('../lib/getPkgInfo.js', () => {
  return {
    ...actualModule,
    getPkgInfo: async (pkg: PackageInfo, _opts: GetPackageInfoOptions): Promise<
    {
      from: string
      description?: string
    } & Omit<LicensePackage, 'belongsTo'>
    > => {
      const packageInfo = {
        from: pkg.name!,
        name: pkg.name!,
        version: pkg.version!,
        description: 'Package Description',
        license: pkg.name === 'bar' ? 'MIT' : 'Unknown',
        licenseContents: pkg.name === 'bar' ? undefined : 'The MIT License',
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: `/path/to/package/${pkg.name!}@${pkg.version!}/node_modules`,
      }

      return packageInfo
    },
  }
})

const { findDependencyLicenses } = await import('@pnpm/deps.compliance.license-scanner')

describe('licences', () => {
  test('findDependencyLicenses()', async () => {
    const lockfile: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            foo: '1.0.0',
          },
          specifiers: {
            foo: '^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        ['foo@1.0.0' as DepPath]: {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }

    const licensePackages = await findDependencyLicenses({
      lockfileDir: '/opt/pnpm',
      manifest: {} as ProjectManifest,
      virtualStoreDir: '/.pnpm',
      registriesByScope: {} as RegistriesByScope,
      wantedLockfile: lockfile,
      storeDir: tmpStoreDir,
      virtualStoreDirMaxLength: 120,
    })

    expect(licensePackages).toEqual([
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '1.0.0',
        name: 'bar',
        license: 'MIT',
        licenseContents: undefined,
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/bar@1.0.0/node_modules',
      },
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '1.0.0',
        name: 'foo',
        license: 'Unknown',
        licenseContents: 'The MIT License',
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/foo@1.0.0/node_modules',
      },
    ] as LicensePackage[])
  })

  test('filterable by includedImporterIds', async () => {
    const lockfile: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            foo: '1.0.0',
          },
          specifiers: {
            foo: '^1.0.0',
          },
        },
        ['packages/a' as ProjectId]: {
          dependencies: {
            bar: '1.0.0',
          },
          specifiers: {
            bar: '^1.0.0',
          },
        },
        ['packages/b' as ProjectId]: {
          dependencies: {
            baz: '1.0.0',
          },
          specifiers: {
            baz: '^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['baz@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'baz-integrity',
          },
        },
        ['bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        ['foo@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }

    const licensePackages = await findDependencyLicenses({
      lockfileDir: '/opt/pnpm',
      manifest: {} as ProjectManifest,
      virtualStoreDir: '/.pnpm',
      registriesByScope: {} as RegistriesByScope,
      wantedLockfile: lockfile,
      storeDir: tmpStoreDir,
      includedImporterIds: ['packages/a'] as ProjectId[],
      virtualStoreDirMaxLength: 120,
    })

    expect(licensePackages).toEqual([
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '1.0.0',
        name: 'bar',
        license: 'MIT',
        licenseContents: undefined,
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/bar@1.0.0/node_modules',
      },
    ] as LicensePackage[])
  })

  test('findDependencyLicenses lists all versions (#7724)', async () => {
    const lockfile: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            foo: '1.0.0',
            bar: '1.0.1',
            baz: '2.0.0',
          },
          specifiers: {
            foo: '^1.0.0',
            bar: '^1.0.1',
            baz: '^2.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['bar@1.0.1' as DepPath]: {
          resolution: {
            integrity: 'bar1-integrity',
          },
        },
        ['bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'bar2-integrity',
          },
        },
        ['baz@2.0.1' as DepPath]: {
          resolution: {
            integrity: 'baz1-integrity',
          },
        },
        ['baz@2.0.0' as DepPath]: {
          resolution: {
            integrity: 'baz2-integrity',
          },
        },
        ['foo@1.0.0' as DepPath]: {
          dependencies: {
            bar: '1.0.0',
            baz: '2.0.1',
          },
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }

    const licensePackages = await findDependencyLicenses({
      lockfileDir: '/opt/pnpm',
      manifest: {} as ProjectManifest,
      virtualStoreDir: '/.pnpm',
      registriesByScope: {} as RegistriesByScope,
      wantedLockfile: lockfile,
      storeDir: tmpStoreDir,
      virtualStoreDirMaxLength: 120,
    })

    expect(licensePackages).toEqual([
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '1.0.0',
        name: 'bar',
        license: 'MIT',
        licenseContents: undefined,
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/bar@1.0.0/node_modules',
      },
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '1.0.1',
        name: 'bar',
        license: 'MIT',
        licenseContents: undefined,
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/bar@1.0.1/node_modules',
      },
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '2.0.0',
        name: 'baz',
        license: 'Unknown',
        licenseContents: 'The MIT License',
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/baz@2.0.0/node_modules',
      },
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '2.0.1',
        name: 'baz',
        license: 'Unknown',
        licenseContents: 'The MIT License',
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/baz@2.0.1/node_modules',
      },
      {
        belongsTo: 'dependencies',
        description: 'Package Description',
        version: '1.0.0',
        name: 'foo',
        license: 'Unknown',
        licenseContents: 'The MIT License',
        author: 'Package Author',
        homepage: 'Homepage',
        repository: 'Repository',
        path: '/path/to/package/foo@1.0.0/node_modules',
      },
    ] as LicensePackage[])
  })

  test('findDependencyLicenses lists versions installed under different aliases', async () => {
    const lockfile: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            prettier: '3.6.2',
            prettier2: 'prettier@2.8.8',
          },
          specifiers: {
            prettier: '3.6.2',
            prettier2: 'npm:prettier@2.8.8',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['prettier@2.8.8' as DepPath]: {
          resolution: {
            integrity: 'prettier2-integrity',
          },
        },
        ['prettier@3.6.2' as DepPath]: {
          resolution: {
            integrity: 'prettier3-integrity',
          },
        },
      },
    }

    const licensePackages = await findDependencyLicenses({
      lockfileDir: '/opt/pnpm',
      manifest: {} as ProjectManifest,
      virtualStoreDir: '/.pnpm',
      registriesByScope: {} as RegistriesByScope,
      wantedLockfile: lockfile,
      storeDir: tmpStoreDir,
      virtualStoreDirMaxLength: 120,
    })

    expect(licensePackages.map(({ name, version }) => ({ name, version }))).toEqual([
      {
        name: 'prettier',
        version: '2.8.8',
      },
      {
        name: 'prettier',
        version: '3.6.2',
      },
    ])
  })

  test('findDependencyLicenses keeps the same version from two registries apart', async () => {
    const lockfile: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            foo: '1.0.0',
            'foo-from-work': 'foo@work:1.0.0',
          },
          specifiers: {
            foo: '^1.0.0',
            'foo-from-work': 'work:foo@^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['foo@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'foo-from-npmjs-integrity',
          },
        },
        ['foo@work:1.0.0' as DepPath]: {
          resolution: {
            integrity: 'foo-from-work-integrity',
          },
        },
      },
    }

    const licensePackages = await findDependencyLicenses({
      lockfileDir: '/opt/pnpm',
      manifest: {} as ProjectManifest,
      virtualStoreDir: '/.pnpm',
      registriesByScope: {} as RegistriesByScope,
      registriesByPrefix: normalizeRegistriesByPrefix({ work: 'https://npm.enterprise.example.com/' }),
      wantedLockfile: lockfile,
      storeDir: tmpStoreDir,
      virtualStoreDirMaxLength: 120,
    })

    // Two distinct artifacts with their own licenses. Keying the dedupe map
    // on a bare `name@version` collapsed them and dropped one entirely.
    expect(licensePackages).toHaveLength(2)
    expect(new Set(licensePackages.map((pkg) => pkg.registryName))).toStrictEqual(new Set([undefined, 'work']))
    expect(new Set(licensePackages.map((pkg) => pkg.name))).toStrictEqual(new Set(['foo']))
  })
})
