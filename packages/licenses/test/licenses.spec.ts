import { licences } from '@pnpm/licenses'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { PackageManifest, ProjectManifest, Registries } from '@pnpm/types'
import { Lockfile } from '@pnpm/lockfile-file'
import { GetPackageInfoFunction } from '../lib/licenses'

const getPackageInfo: GetPackageInfoFunction = async (pkg) => {
  return {
    packageManifest: {} as unknown as PackageManifest,
    packageInfo: {
      from: pkg.name,
      path: pkg.prefix,
      version: pkg.version,
      description: 'Package Description',
      license: pkg.name === 'bar' ? 'MIT' : 'Unknown',
      licenseContents: pkg.name === 'bar' ? undefined : 'The MIT License',
      author: 'Package Author',
      homepage: 'Homepage',
      repository: 'https://github.com/pnpm/pnpm.git',
    },
  }
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
      prefix: '/opt/pnpm',
      virtualStoreDir: '/.pnpm',
      registries: {} as Registries,
      wantedLockfile: lockfile,
      getPackageInfo,
    })

    expect(licensePackages).toEqual([
      {
        belongsTo: 'dependencies',
        version: '1.0.0',
        packageManifest: {},
        packageName: 'bar',
        license: 'MIT',
        licenseContents: undefined,
        author: 'Package Author',
        packageDirectory: '/opt/pnpm/.pnpm/bar@1.0.0/node_modules',
      },
      {
        belongsTo: 'dependencies',
        version: '1.0.0',
        packageManifest: {},
        packageName: 'foo',
        license: 'Unknown',
        licenseContents: 'The MIT License',
        author: 'Package Author',
        packageDirectory: '/opt/pnpm/.pnpm/foo@1.0.0/node_modules',
      },
    ])
  })
})
