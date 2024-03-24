import { convertToLockfileFile, convertToLockfileObject } from '../lib/lockfileFormatConverters'

test('convertToLockfileFile()', () => {
  const lockfileV5 = {
    lockfileVersion: '7.0',
    importers: {
      project1: {
        specifiers: {
          foo: '^1.0.0',
          bar: '^1.0.0',
          qar: '^1.0.0',
          tarball: '^1.0.0',
        },
        dependencies: {
          foo: '1.0.0',
          tarball: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
        devDependencies: {
          bar: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
        },
        optionalDependencies: {
          qar: 'reg.com/qar@1.0.0',
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileV6 = {
    lockfileVersion: '7.0',
    importers: {
      project1: {
        dependencies: {
          foo: {
            specifier: '^1.0.0',
            version: '1.0.0',
          },
          tarball: {
            specifier: '^1.0.0',
            version: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          },
        },
        devDependencies: {
          bar: {
            specifier: '^1.0.0',
            version: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
          },
        },
        optionalDependencies: {
          qar: {
            specifier: '^1.0.0',
            version: 'reg.com/qar@1.0.0',
          },
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
    snapshots: {
      '/foo@1.0.0': {},
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {},
      'reg.com/qar@1.0.0': {},
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {},
    },
  }
  expect(convertToLockfileFile(lockfileV5, { forceSharedFormat: false })).toEqual(lockfileV6)
  expect(convertToLockfileObject(lockfileV6)).toEqual(lockfileV5)
})

test('convertToLockfileFile() with lockfile v6', () => {
  const lockfileV5 = {
    lockfileVersion: '7.0',
    importers: {
      project1: {
        specifiers: {
          foo: '^1.0.0',
          bar: '^1.0.0',
          qar: '^1.0.0',
          tarball: '^1.0.0',
        },
        dependencies: {
          foo: '1.0.0',
          tarball: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
        devDependencies: {
          bar: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
        },
        optionalDependencies: {
          qar: 'reg.com/qar@1.0.0',
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileV6 = {
    lockfileVersion: '7.0',
    importers: {
      project1: {
        dependencies: {
          foo: {
            specifier: '^1.0.0',
            version: '1.0.0',
          },
          tarball: {
            specifier: '^1.0.0',
            version: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          },
        },
        devDependencies: {
          bar: {
            specifier: '^1.0.0',
            version: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
          },
        },
        optionalDependencies: {
          qar: {
            specifier: '^1.0.0',
            version: 'reg.com/qar@1.0.0',
          },
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
    snapshots: {
      '/foo@1.0.0': {},
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {},
      'reg.com/qar@1.0.0': {},
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {},
    },
  }
  expect(convertToLockfileFile(lockfileV5, { forceSharedFormat: false })).toEqual(lockfileV6)
  expect(convertToLockfileObject(lockfileV6)).toEqual(lockfileV5)
})

test('convertToLockfileObject() converts package IDs', () => {
  const lockfileFile = {
    lockfileVersion: '6.0',
    importers: {
      project1: {
        dependencies: {
          'is-positive': {
            specifier: 'github:kevva/is-positive',
            version: 'github.com/kevva/is-positive/97edff6f525f192a3f83cea1944765f769ae2678(@babel/core@2.0.0)',
          },
          tarball: {
            specifier: '^1.0.0',
            version: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          },
          'git-hosted': {
            specifier: 'gitlab:pnpm/git-resolver',
            version: 'gitlab/pnpm/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc(foo@1.0.0)',
          },
          'is-odd': {
            specifier: '1.0.0',
            version: '1.0.0',
          },
        },
      },
    },
    packages: {
      'github.com/kevva/is-positive/97edff6f525f192a3f83cea1944765f769ae2678(@babel/core@2.0.0)': {
        name: 'is-positive',
        resolution: { tarball: 'https://codeload.github.com/kevva/is-positive/tar.gz/97edff6f525f192a3f83cea1944765f769ae2678' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        name: 'is-positive',
        resolution: { tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz', integrity: '' },
      },
      'gitlab/pnpm/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc(foo@1.0.0)': {
        id: 'gitlab/pnpm/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc',
        name: 'git-hosted',
        resolution: {
          commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
          repo: 'ssh://git@gitlab/pnpm/git-resolver',
          type: 'git',
        },
      },
      '/is-odd@1.0.0': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileObject = {
    lockfileVersion: '6.0',
    importers: {
      project1: {
        dependencies: {
          'is-positive': 'https://codeload.github.com/kevva/is-positive/tar.gz/97edff6f525f192a3f83cea1944765f769ae2678(@babel/core@2.0.0)',
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          'git-hosted': 'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc(foo@1.0.0)',
          'is-odd': '1.0.0',
        },
        specifiers: {
          'is-positive': 'github:kevva/is-positive',
          tarball: '^1.0.0',
          'git-hosted': 'gitlab:pnpm/git-resolver',
          'is-odd': '1.0.0',
        },
      },
    },
    packages: {
      'https://codeload.github.com/kevva/is-positive/tar.gz/97edff6f525f192a3f83cea1944765f769ae2678(@babel/core@2.0.0)': {
        name: 'is-positive',
        resolution: { tarball: 'https://codeload.github.com/kevva/is-positive/tar.gz/97edff6f525f192a3f83cea1944765f769ae2678' },
      },
      'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        name: 'is-positive',
        resolution: { tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz', integrity: '' },
      },
      'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc(foo@1.0.0)': {
        id: 'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
        name: 'git-hosted',
        resolution: {
          commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
          repo: 'ssh://git@gitlab/pnpm/git-resolver',
          type: 'git',
        },
      },
      'is-odd@1.0.0': {
        resolution: { integrity: '' },
      },
    },
  }
  expect(convertToLockfileObject(lockfileFile as any)).toEqual(lockfileObject) // eslint-disable-line
})
