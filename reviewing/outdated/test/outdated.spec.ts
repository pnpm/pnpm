import { outdated } from '../lib/outdated'

async function getLatestManifest (packageName: string) {
  return ({
    'deprecated-pkg': {
      deprecated: 'This package is deprecated',
      name: 'deprecated-pkg',
      version: '1.0.0',
    },
    'is-negative': {
      name: 'is-negative',
      version: '2.1.0',
    },
    'is-positive': {
      name: 'is-positive',
      version: '3.1.0',
    },
    'pkg-with-1-dep': {
      name: 'pkg-with-1-dep',
      version: '1.0.0',
    },
  })[packageName] ?? null
}

test('outdated()', async () => {
  const outdatedPkgs = await outdated({
    currentLockfile: {
      importers: {
        '.': {
          dependencies: {
            'from-github': 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
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
        'is-negative@2.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        'is-positive@1.0.0': {
          resolution: {
            integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          },
        },
        'from-github@https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b4': {
          version: '1.1.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
      },
    },
    getLatestManifest,
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
            'from-github': 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'from-github-2': 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
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
        'is-negative@1.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        'is-positive@3.1.0': {
          resolution: {
            integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
          },
        },
        'from-github-2@https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3': {
          version: '1.0.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
        'from-github@https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3': {
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
  expect(outdatedPkgs).toStrictEqual([
    {
      alias: 'from-github',
      belongsTo: 'dependencies',
      current: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
      latestManifest: undefined,
      packageName: 'from-github',
      wanted: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
      workspace: 'wanted-shrinkwrap',
    },
    {
      alias: 'from-github-2',
      belongsTo: 'dependencies',
      current: undefined,
      latestManifest: undefined,
      packageName: 'from-github-2',
      wanted: 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
      workspace: 'wanted-shrinkwrap',
    },
    {
      alias: 'is-negative',
      belongsTo: 'devDependencies',
      current: '1.0.0',
      latestManifest: {
        name: 'is-negative',
        version: '2.1.0',
      },
      packageName: 'is-negative',
      wanted: '1.1.0',
      workspace: 'wanted-shrinkwrap',
    },
    {
      alias: 'is-positive',
      belongsTo: 'devDependencies',
      current: '1.0.0',
      latestManifest: {
        name: 'is-positive',
        version: '3.1.0',
      },
      packageName: 'is-positive',
      wanted: '3.1.0',
      workspace: 'wanted-shrinkwrap',
    },
  ])
})

test('outdated() should return deprecated package even if its current version is latest', async () => {
  const lockfile = {
    importers: {
      '.': {
        dependencies: {
          'deprecated-pkg': '1.0.0',
        },
        specifiers: {
          'deprecated-pkg': '^1.0.0',
        },
      },
    },
    lockfileVersion: 5,
    packages: {
      'deprecated-pkg@1.0.0': {
        resolution: {
          integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
        },
      },
    },
  }
  const outdatedPkgs = await outdated({
    currentLockfile: lockfile,
    getLatestManifest,
    lockfileDir: 'project',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        'deprecated-pkg': '1.0.0',
      },
    },
    prefix: 'project',
    wantedLockfile: lockfile,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(outdatedPkgs).toStrictEqual([
    {
      alias: 'deprecated-pkg',
      belongsTo: 'dependencies',
      current: '1.0.0',
      latestManifest: {
        deprecated: 'This package is deprecated',
        name: 'deprecated-pkg',
        version: '1.0.0',
      },
      packageName: 'deprecated-pkg',
      wanted: '1.0.0',
      workspace: 'wanted-shrinkwrap',
    },
  ])
})

test('using a matcher', async () => {
  const outdatedPkgs = await outdated({
    currentLockfile: {
      importers: {
        '.': {
          dependencies: {
            'from-github': 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
            'is-negative': '1.0.0',
            'is-positive': '1.0.0',
            'linked-1': 'link:../linked-1',
            'linked-2': 'file:../linked-2',
          },
          specifiers: {
            'is-negative': '^2.1.0',
            'is-positive': '^1.0.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        'is-negative@2.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        'is-positive@1.0.0': {
          resolution: {
            integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          },
        },
        'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4': {
          name: 'from-github',
          version: '1.1.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
      },
    },
    getLatestManifest,
    lockfileDir: 'wanted-shrinkwrap',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        'is-negative': '^2.1.0',
        'is-positive': '^3.1.0',
      },
    },
    match: (dependencyName) => dependencyName === 'is-negative',
    prefix: 'wanted-shrinkwrap',
    wantedLockfile: {
      importers: {
        '.': {
          dependencies: {
            'from-github': 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'from-github-2': 'github.com/blabla/from-github-2/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'is-negative': '1.1.0',
            'is-positive': '3.1.0',
            'linked-1': 'link:../linked-1',
            'linked-2': 'file:../linked-2',
          },
          specifiers: {
            'is-negative': '^2.1.0',
            'is-positive': '^3.1.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        'is-negative@1.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        'is-positive@3.1.0': {
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
  expect(outdatedPkgs).toStrictEqual([
    {
      alias: 'is-negative',
      belongsTo: 'dependencies',
      current: '1.0.0',
      latestManifest: {
        name: 'is-negative',
        version: '2.1.0',
      },
      packageName: 'is-negative',
      wanted: '1.1.0',
      workspace: 'wanted-shrinkwrap',
    },
  ])
})

test('outdated() aliased dependency', async () => {
  const outdatedPkgs = await outdated({
    currentLockfile: {
      importers: {
        '.': {
          dependencies: {
            positive: 'is-positive@1.0.0',
          },
          specifiers: {
            positive: 'npm:is-positive@^1.0.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        'is-positive@1.0.0': {
          resolution: {
            integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          },
        },
      },
    },
    getLatestManifest,
    lockfileDir: 'project',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        positive: 'npm:is-positive@^3.1.0',
      },
    },
    prefix: 'project',
    wantedLockfile: {
      importers: {
        '.': {
          dependencies: {
            positive: 'is-positive@3.1.0',
          },
          specifiers: {
            positive: 'npm:is-positive@^3.1.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        'is-positive@3.1.0': {
          resolution: {
            integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
          },
        },
      },
    },
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(outdatedPkgs).toStrictEqual([
    {
      alias: 'positive',
      belongsTo: 'dependencies',
      current: '1.0.0',
      latestManifest: {
        name: 'is-positive',
        version: '3.1.0',
      },
      packageName: 'is-positive',
      wanted: '3.1.0',
      workspace: 'wanted-shrinkwrap',
    },
  ])
})

test('a dependency is not outdated if it is newer than the latest version', async () => {
  const lockfile = {
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
          foo2: '2.0.0-0',
          foo3: '2.0.0',
        },
        specifiers: {
          foo: '^1.0.0',
          foo2: '2.0.0-0',
          foo3: '2.0.0',
        },
      },
    },
    lockfileVersion: 5,
    packages: {
      'foo@1.0.0': {
        resolution: {
          integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
        },
      },
      'foo2@2.0.0-0': {
        resolution: {
          integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
        },
      },
      'foo3@2.0.0': {
        resolution: {
          integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
        },
      },
    },
  }
  const outdatedPkgs = await outdated({
    currentLockfile: lockfile,
    getLatestManifest: async (packageName) => {
      switch (packageName) {
      case 'foo':
        return {
          name: 'foo',
          version: '0.1.0',
        }
      case 'foo2':
        return {
          name: 'foo2',
          version: '1.0.0',
        }
      case 'foo3':
        return {
          name: 'foo3',
          version: '2.0.0',
        }
      }
      return null
    },
    lockfileDir: 'project',
    manifest: {
      name: 'pkg',
      version: '1.0.0',

      dependencies: {
        foo: '^1.0.0',
        foo2: '2.0.0-0',
        foo3: '2.0.0',
      },
    },
    prefix: 'project',
    wantedLockfile: lockfile,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(outdatedPkgs).toStrictEqual([])
})

test('outdated() should [] when there is no dependency', async () => {
  const outdatedPkgs = await outdated({
    currentLockfile: null,
    getLatestManifest: async () => {
      return null
    },
    lockfileDir: 'project',
    manifest: {
      name: 'pkg',
      version: '1.0.0',
    },
    prefix: 'project',
    wantedLockfile: null,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(outdatedPkgs).toStrictEqual([])
})

test('should ignore dependencies as expected', async () => {
  const outdatedPkgs = await outdated({
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
        'is-negative@2.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        'is-positive@1.0.0': {
          resolution: {
            integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          },
        },
        'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4': {
          name: 'from-github',
          version: '1.1.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
      },
    },
    getLatestManifest,
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
            'from-github': 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'from-github-2': 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
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
            'from-github': 'https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'from-github-2': 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
            'is-negative': '^2.1.0',
            'is-positive': '^3.1.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        'is-negative@1.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        'is-positive@3.1.0': {
          resolution: {
            integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
          },
        },
        'from-github-2@https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3': {
          version: '1.0.0',

          resolution: {
            tarball: 'https://codeload.github.com/blabla/from-github-2/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
          },
        },
        'from-github@https://codeload.github.com/blabla/from-github/tar.gz/d5f8d5500f7faf593d32e134c1b0043ff69151b3': {
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
    ignoreDependencies: [
      'from-*',
      'is-negative',
    ],
  })
  expect(outdatedPkgs).toStrictEqual([
    {
      alias: 'is-positive',
      belongsTo: 'devDependencies',
      current: '1.0.0',
      latestManifest: {
        name: 'is-positive',
        version: '3.1.0',
      },
      packageName: 'is-positive',
      wanted: '3.1.0',
      workspace: 'wanted-shrinkwrap',
    },
  ])
})
