import { licences } from '@pnpm/licenses'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { ProjectManifest, Registries } from '@pnpm/types'
import { Lockfile } from '@pnpm/lockfile-file'
import { GetPackageInfoFunction, LicensePackage } from '../lib/licenses'

const getPackageInfo: GetPackageInfoFunction = async (pkg, _opts): Promise<
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
    vendorName: 'Package Author',
    vendorUrl: 'Homepage',
    vendorRepository: 'Repository',
    path: `/path/to/package/${pkg.name!}@${pkg.version!}/node_modules`,
  }

  return packageInfo
}

describe('licences', () => {
  test('licences()', async () => {
    const lockfile: Lockfile = {
      importers: {
        '.': {
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
        '/bar/1.0.0': {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        '/foo/1.0.0': {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }

    const licensePackages = await licences({
      lockfileDir: '/opt/pnpm',
      manifest: {} as ProjectManifest,
      virtualStoreDir: '/.pnpm',
      registries: {} as Registries,
      wantedLockfile: lockfile,
      getPackageInfo,
      storeDir: '/opt/.pnpm',
    })

    expect(licensePackages).toEqual([
      {
        belongsTo: 'dependencies',
        version: '1.0.0',
        name: 'bar',
        license: 'MIT',
        licenseContents: undefined,
        vendorName: 'Package Author',
        vendorUrl: 'Homepage',
        vendorRepository: 'Repository',
        path: '/path/to/package/bar@1.0.0/node_modules',
      },
      {
        belongsTo: 'dependencies',
        version: '1.0.0',
        name: 'foo',
        license: 'Unknown',
        licenseContents: 'The MIT License',
        vendorName: 'Package Author',
        vendorUrl: 'Homepage',
        vendorRepository: 'Repository',
        path: '/path/to/package/foo@1.0.0/node_modules',
      },
    ] as LicensePackage[])
  })
})
