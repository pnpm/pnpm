import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type LockfileObject } from '@pnpm/lockfile.fs'
import { type ProjectId } from '@pnpm/types'
import { assertLockfilesEqual } from '../src/assertLockfilesEqual'

test('if wantedLockfile does not have any specifier, currentLockfile is allowed to be null', () => {
  assertLockfilesEqual(null, {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: {},
      },
    },
  }, '<LOCKFILE_DIR>')
})

test('should throw if wantedLockfile has specifiers but currentLockfile is null', () => {
  // eslint-disable-next-line @typescript-eslint/no-confusing-void-expression
  expect(() => assertLockfilesEqual(null, {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: {
          foo: '^1.0.0',
        },
        dependencies: {
          foo: '1.0.1',
        },
      },
    },
  }, '<LOCKFILE_DIR>')).toThrow('Project . requires dependencies but none was installed.')
})

test('should not throw if wantedLockfile and currentLockfile are equal', () => {
  const lockfile = (): LockfileObject => ({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: {
          foo: '^1.0.0',
        },
        dependencies: {
          foo: '1.0.1',
        },
      },
    },
  })
  assertLockfilesEqual(lockfile(), lockfile(), '<LOCKFILE_DIR>')
})

test('should throw if wantedLockfile and currentLockfile are not equal', () => {
  // eslint-disable-next-line @typescript-eslint/no-confusing-void-expression
  expect(() => assertLockfilesEqual(
    {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        ['.' as ProjectId]: {
          specifiers: {
            foo: '^1.0.0',
          },
          dependencies: {
            foo: '1.0.1',
          },
        },
      },
    },
    {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        ['.' as ProjectId]: {
          specifiers: {
            foo: '^1.0.0',
          },
          dependencies: {
            foo: '1.1.0',
          },
        },
      },
    },
    '<LOCKFILE_DIR>')
  ).toThrow('The installed dependencies in the modules directory is not up-to-date with the lockfile in <LOCKFILE_DIR>.')
})
