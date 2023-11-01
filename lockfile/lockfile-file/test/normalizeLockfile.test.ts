import { LOCKFILE_VERSION } from '@pnpm/constants'
import { normalizeLockfile } from '../lib/write'

test('empty overrides and neverBuiltDependencies are removed during lockfile normalization', () => {
  expect(normalizeLockfile({
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
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
        },
      },
    },
  }, {
    forceSharedFormat: false,
    includeEmptySpecifiersField: false,
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

test('empty specifiers field is preserved', () => {
  expect(normalizeLockfile({
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
    importers: {
      foo: {},
    },
  }, {
    forceSharedFormat: false,
    includeEmptySpecifiersField: true,
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {},
    },
  })
})

test('empty specifiers field is removed', () => {
  expect(normalizeLockfile({
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
    importers: {
      foo: {},
    },
  }, {
    forceSharedFormat: false,
    includeEmptySpecifiersField: false,
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {},
    },
  })
})

test('redundant fields are removed from "time"', () => {
  expect(normalizeLockfile({
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
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
            version: '1.0.0_react@18.0.0',
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
      '/bar/1.0.0': '2021-02-11T22:54:29.120Z',
      '/foo/1.0.0': '2021-02-11T22:54:29.120Z',
      '/qar/1.0.0': '2021-02-11T22:54:29.120Z',
      '/zoo/1.0.0': '2021-02-11T22:54:29.120Z',
    },
  }, {
    forceSharedFormat: false,
    includeEmptySpecifiersField: false,
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
            version: '1.0.0_react@18.0.0',
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
      '/bar/1.0.0': '2021-02-11T22:54:29.120Z',
      '/foo/1.0.0': '2021-02-11T22:54:29.120Z',
      '/qar/1.0.0': '2021-02-11T22:54:29.120Z',
    },
  })
})
