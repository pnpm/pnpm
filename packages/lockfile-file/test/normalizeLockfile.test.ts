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
          bar: 'link:../bar',
        },
        specifiers: {
          bar: 'link:../bar',
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
          bar: 'link:../bar',
        },
        specifiers: {
          bar: 'link:../bar',
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
      foo: {
        specifiers: {},
      },
    },
  }, {
    forceSharedFormat: false,
    includeEmptySpecifiersField: true,
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        specifiers: {},
      },
    },
  })
})

test('empty specifiers field is removed', () => {
  expect(normalizeLockfile({
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
    importers: {
      foo: {
        specifiers: {},
      },
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
