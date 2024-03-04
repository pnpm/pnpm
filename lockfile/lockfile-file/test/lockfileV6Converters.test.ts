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
