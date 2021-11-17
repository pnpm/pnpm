import { LOCKFILE_VERSION } from '@pnpm/constants'
import { normalizeLockfile } from '@pnpm/lockfile-file/lib/write'

test('empty overrides and neverBuiltDependencies are removed during lockfile normalization', () => {
  expect(normalizeLockfile({
    lockfileVersion: LOCKFILE_VERSION,
    // but this should be preserved.
    onlyBuiltDependencies: [],
    overrides: {},
    neverBuiltDependencies: [],
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
  }, false)).toStrictEqual({
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
