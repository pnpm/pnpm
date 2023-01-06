import { convertToInlineSpecifiersFormat, revertFromInlineSpecifiersFormat } from '../lib/experiments/inlineSpecifiersLockfileConverters'

test('convertToInlineSpecifiersFormat()', () => {
  const lockfileV5 = {
    lockfileVersion: 5.0,
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
          bar: '/@bar/bar/1.0.0_@babel+core@2.0.0',
        },
        optionalDependencies: {
          qar: 'reg.com/qar/1.0.0',
        },
      },
    },
    packages: {
      '/foo/1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar/1.0.0_@babel+core@2.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar/1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileV6 = {
    lockfileVersion: '5-inlineSpecifiers',
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
            version: '/@bar/bar/1.0.0_@babel+core@2.0.0',
          },
        },
        optionalDependencies: {
          qar: {
            specifier: '^1.0.0',
            version: 'reg.com/qar/1.0.0',
          },
        },
      },
    },
    packages: {
      '/foo/1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar/1.0.0_@babel+core@2.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar/1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  expect(convertToInlineSpecifiersFormat(lockfileV5)).toEqual(lockfileV6)
  expect(revertFromInlineSpecifiersFormat(lockfileV6)).toEqual(lockfileV5)
})

test('convertToInlineSpecifiersFormat() with lockfile v6', () => {
  const lockfileV5 = {
    lockfileVersion: '6.0',
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
          bar: '/@bar/bar/1.0.0_@babel+core@2.0.0',
        },
        optionalDependencies: {
          qar: 'reg.com/qar/1.0.0',
        },
      },
    },
    packages: {
      '/foo/1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar/1.0.0_@babel+core@2.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar/1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileV6 = {
    lockfileVersion: '6.0',
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
            version: '/@bar/bar@1.0.0_@babel+core@2.0.0',
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
      '/@bar/bar@1.0.0_@babel+core@2.0.0': {
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
  expect(convertToInlineSpecifiersFormat(lockfileV5)).toEqual(lockfileV6)
  expect(revertFromInlineSpecifiersFormat(lockfileV6)).toEqual(lockfileV5)
})
