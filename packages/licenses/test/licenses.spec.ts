import { PackageManifest } from '@pnpm/types'
import { licences } from '../lib/licenses'
import path from 'node:path'

async function fetchPackageInfo (pkg: {
  alias: string
  name: string
  version: string
  prefix: string
}) {
  const packageModulePath = path.join(pkg.prefix, 'node_modules', pkg.name)

  return {
    packageManifest: {} as PackageManifest,
    packageInfo: {
      name: pkg.name,
      packageDirectory: packageModulePath,
      license: 'MIT',
      licenseContents: '',
      author: 'Author Name',
      version: pkg.version,
    } as unknown,
  }
}

test('licences()', async () => {
  const licencesPkgs = await licences({
    getPackageInfo: fetchPackageInfo as never,
    currentLockfile: {
      importers: {
        '.': {
          dependencies: {
            'from-github': 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
          },
          devDependencies: {
            'is-negative': '1.0.0',
            'is-positive': '1.0.0',
          },
          optionalDependencies: {
            'linked-1': 'link:../linked-1',
            'linked-2': 'file:../linked-2',
          },
          specifiers: {
            'from-github': 'github:blabla/from-github#d5f8d5500f7faf593d32e134c1b0043ff69151b4',
            'is-negative': '^2.1.0',
            'is-positive': '^1.0.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        '/is-negative/2.1.0': {
          dev: true,
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        '/is-positive/1.0.0': {
          dev: true,
          resolution: {
            integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          },
        },
        'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4': {
          name: 'from-github',
          version: '1.1.0',

          dev: false,
          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
      },
    },
    lockfileDir: 'project',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',
      dependencies: {
        'from-github': 'github:blabla/from-github#d5f8d5500f7faf593d32e134c1b0043ff69151b4',
        'from-github-2': 'github:blabla/from-github-2#d5f8d5500f7faf593d32e134c1b0043ff69151b4',
      },
      devDependencies: {
        'is-negative': '^2.1.0',
        'is-positive': '^3.1.0',
      },
    },
    prefix: 'project',
    wantedLockfile: {
      importers: {
        '.': {
          dependencies: {
            'from-github': 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'from-github-2': 'github.com/blabla/from-github-2/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
          devDependencies: {
            'is-negative': '1.1.0',
            'is-positive': '3.1.0',
          },
          optionalDependencies: {
            'linked-1': 'link:../linked-1',
            'linked-2': 'file:../linked-2',
          },
          specifiers: {
            'from-github': 'github:blabla/from-github#d5f8d5500f7faf593d32e134c1b0043ff69151b4',
            'from-github-2': 'github:blabla/from-github-2#d5f8d5500f7faf593d32e134c1b0043ff69151b4',
            'is-negative': '^2.1.0',
            'is-positive': '^3.1.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        '/is-negative/1.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        '/is-positive/3.1.0': {
          resolution: {
            integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
          },
        },
        'github.com/blabla/from-github-2/d5f8d5500f7faf593d32e134c1b0043ff69151b3': {
          name: 'from-github-2',
          version: '1.0.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
        'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b3': {
          name: 'from-github',
          version: '1.0.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
      },
    },
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(licencesPkgs).toStrictEqual([
    {
      alias: 'from-github',
      belongsTo: 'dependencies',
      packageName: 'from-github',
      author: 'Author Name',
      license: 'MIT',
      licenseContents: '',
      packageDirectory: undefined,
      packageManifest: {} as PackageManifest,
      version: '1.0.0',
    },
    {
      alias: 'from-github-2',
      belongsTo: 'dependencies',
      packageName: 'from-github-2',
      author: 'Author Name',
      license: 'MIT',
      licenseContents: '',
      packageDirectory: undefined,
      packageManifest: {} as PackageManifest,
      version: '1.0.0',
    },
    {
      alias: 'is-negative',
      belongsTo: 'devDependencies',
      packageName: 'is-negative',
      author: 'Author Name',
      license: 'MIT',
      licenseContents: '',
      packageDirectory: undefined,
      packageManifest: {} as PackageManifest,
      version: '1.1.0',
    },
    {
      alias: 'is-positive',
      belongsTo: 'devDependencies',
      packageName: 'is-positive',
      author: 'Author Name',
      license: 'MIT',
      licenseContents: '',
      packageDirectory: undefined,
      packageManifest: {} as PackageManifest,
      version: '3.1.0',
    },
  ])
})
