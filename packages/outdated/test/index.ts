///<reference path="../../../typings/index.d.ts"/>
import outdated, { forPackages as outdatedForPackages } from '@pnpm/outdated'
import test = require('tape')

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
  })[packageName] || null
}

test('outdated()', async (t) => {
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
            integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
    getLatestManifest,
    lockfileDirectory: 'project',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

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
            integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
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
  })
  t.deepEqual(outdatedPkgs, [
    {
      alias: 'from-github',
      belongsTo: 'dependencies',
      current: 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
      latestManifest: undefined,
      packageName: 'from-github',
      wanted: 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
    },
    {
      alias: 'from-github-2',
      belongsTo: 'dependencies',
      current: undefined,
      latestManifest: undefined,
      packageName: 'from-github-2',
      wanted: 'github.com/blabla/from-github-2/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
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
    },
  ])
  t.end()
})

test('outdated() should return deprecated package even if its current version is latest', async (t) => {
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
      '/deprecated-pkg/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
        },
      },
    },
  }
  const outdatedPkgs = await outdated({
    currentLockfile: lockfile,
    getLatestManifest,
    lockfileDirectory: 'project',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        'deprecated-pkg': '1.0.0',
      },
    },
    prefix: 'project',
    wantedLockfile: lockfile,
  })
  t.deepEqual(outdatedPkgs, [
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
    },
  ])
  t.end()
})

test('forPackages()', async (t) => {
  const outdatedPkgs = await outdatedForPackages(['is-negative'], {
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
        '/is-negative/2.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        '/is-positive/1.0.0': {
          resolution: {
            integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
    lockfileDirectory: 'wanted-shrinkwrap',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        'is-negative': '^2.1.0',
        'is-positive': '^3.1.0',
      },
    },
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
        '/is-negative/1.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        '/is-positive/3.1.0': {
          resolution: {
            integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
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
  })
  t.deepEqual(outdatedPkgs, [
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
    },
  ])
  t.end()
})

test('forPackages() by pattern', async (t) => {
  const outdatedPkgs = await outdatedForPackages(['*-negative'], {
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
        '/is-negative/2.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        '/is-positive/1.0.0': {
          resolution: {
            integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
    lockfileDirectory: 'wanted-shrinkwrap',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        'is-negative': '^2.1.0',
        'is-positive': '^3.1.0',
      },
    },
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
        '/is-negative/1.1.0': {
          resolution: {
            integrity: 'sha1-8Nhjd6oVpkw0lh84rCqb4rQKEYc=',
          },
        },
        '/is-positive/3.1.0': {
          resolution: {
            integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
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
  })
  t.deepEqual(outdatedPkgs, [
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
    },
  ])
  t.end()
})

test('outdated() aliased dependency', async (t) => {
  const outdatedPkgs = await outdated({
    currentLockfile: {
      importers: {
        '.': {
          dependencies: {
            'positive': '/is-positive/1.0.0',
          },
          specifiers: {
            'positive': 'npm:is-positive@^1.0.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        '/is-positive/1.0.0': {
          resolution: {
            integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
          },
        },
      },
    },
    getLatestManifest,
    lockfileDirectory: 'project',
    manifest: {
      name: 'wanted-shrinkwrap',
      version: '1.0.0',

      dependencies: {
        'positive': 'npm:is-positive@^3.1.0',
      },
    },
    prefix: 'project',
    wantedLockfile: {
      importers: {
        '.': {
          dependencies: {
            'positive': '/is-positive/3.1.0',
          },
          specifiers: {
            'positive': 'npm:is-positive@^3.1.0',
          },
        },
      },
      lockfileVersion: 5,
      packages: {
        '/is-positive/3.1.0': {
          resolution: {
            integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
          },
        },
      },
    },
  })
  t.deepEqual(outdatedPkgs, [
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
    },
  ])
  t.end()
})
