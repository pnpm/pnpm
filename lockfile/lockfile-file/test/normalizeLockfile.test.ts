import { LOCKFILE_VERSION } from '@pnpm/constants'
import { convertToLockfileFile } from '../lib/lockfileFormatConverters'

test('empty overrides and neverBuiltDependencies are removed during lockfile normalization', () => {
  expect(convertToLockfileFile({
    lockfileVersion: LOCKFILE_VERSION,
    // but this should be preserved.
    onlyBuiltDependencies: [],
    overrides: {},
    neverBuiltDependencies: [],
    patchedDependencies: {},
    packages: {},
    importers: {
      foo: {
        dependencies: {
          bar: 'link:../bar',
        },
        specifiers: {
          bar: 'link:../bar',
        },
      },
    },
  }, {
    forceSharedFormat: false,
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    onlyBuiltDependencies: [],
    importers: {
      foo: {
        dependencies: {
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
        },
      },
    },
  })
})

test('redundant fields are removed from "time"', () => {
  expect(convertToLockfileFile({
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
    importers: {
      foo: {
        dependencies: {
          bar: '1.0.0',
        },
        devDependencies: {
          foo: '1.0.0(react@18.0.0)',
        },
        optionalDependencies: {
          qar: '1.0.0',
        },
        specifiers: {
          bar: '1.0.0',
          foo: '1.0.0',
          qar: '1.0.0',
        },
      },
    },
    time: {
      '/bar@1.0.0': '2021-02-11T22:54:29.120Z',
      '/foo@1.0.0': '2021-02-11T22:54:29.120Z',
      '/qar@1.0.0': '2021-02-11T22:54:29.120Z',
      '/zoo@1.0.0': '2021-02-11T22:54:29.120Z',
    },
  }, {
    forceSharedFormat: false,
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          bar: {
            version: '1.0.0',
            specifier: '1.0.0',
          },
        },
        devDependencies: {
          foo: {
            version: '1.0.0(react@18.0.0)',
            specifier: '1.0.0',
          },
        },
        optionalDependencies: {
          qar: {
            version: '1.0.0',
            specifier: '1.0.0',
          },
        },
      },
    },
    time: {
      '/bar@1.0.0': '2021-02-11T22:54:29.120Z',
      '/foo@1.0.0': '2021-02-11T22:54:29.120Z',
      '/qar@1.0.0': '2021-02-11T22:54:29.120Z',
    },
  })
})
