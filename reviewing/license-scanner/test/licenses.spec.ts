import { findDependencyLicenses } from '@pnpm/license-scanner'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type DepPath, type ProjectManifest, type Registries, type ProjectId } from '@pnpm/types'
import { type Lockfile } from '@pnpm/lockfile-file'
import { type LicensePackage } from '../lib/licenses'
import { type GetPackageInfoOptions, type PackageInfo } from '../lib/getPkgInfo'

jest.mock('../lib/getPkgInfo', () => {
  const actualModule = jest.requireActual('../lib/getPkgInfo')
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

describe('licences', () => {
  test('findDependencyLicenses()', async () => {
    const lockfile: Lockfile = {
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
      registries: {} as Registries,
      wantedLockfile: lockfile,
      storeDir: '/opt/.pnpm',
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
    const lockfile: Lockfile = {
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
      registries: {} as Registries,
      wantedLockfile: lockfile,
      storeDir: '/opt/.pnpm',
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
    const lockfile: Lockfile = {
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
      registries: {} as Registries,
      wantedLockfile: lockfile,
      storeDir: '/opt/.pnpm',
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
})
