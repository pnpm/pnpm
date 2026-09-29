import { describe, expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

import { assertLockfilesEqual } from '../src/assertLockfilesEqual.js'

test('if wantedLockfile does not have any specifier, currentLockfile is allowed to be null', () => {
  assertLockfilesEqual(null, {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers: {},
      },
    },
  }, { wantedLockfileDir: '<LOCKFILE_DIR>' })
})

test('should throw if wantedLockfile has specifiers but currentLockfile is null', () => {
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
  }, { wantedLockfileDir: '<LOCKFILE_DIR>' })).toThrow('Project . requires dependencies but none was installed.')
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
  assertLockfilesEqual(lockfile(), lockfile(), { wantedLockfileDir: '<LOCKFILE_DIR>' })
})

test('should throw if wantedLockfile and currentLockfile are not equal', () => {
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
    { wantedLockfileDir: '<LOCKFILE_DIR>' })
  ).toThrow('The installed dependencies in the modules directory is not up-to-date with the lockfile in <LOCKFILE_DIR>.')
})

describe('after a filtered install', () => {
  const wantedLockfile = (): LockfileObject => ({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['packages/a' as ProjectId]: {
        specifiers: { foo: '1.0.0' },
        dependencies: { foo: '1.0.0' },
      },
      ['packages/b' as ProjectId]: {
        specifiers: { bar: '1.0.0' },
        dependencies: { bar: '1.0.0' },
      },
    },
    packages: {
      ['bar@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-bbb' } },
      ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-aaa' } },
    },
  })
  const currentLockfileOfPackageA = (): LockfileObject => {
    const lockfile = wantedLockfile()
    delete lockfile.packages!['bar@1.0.0' as DepPath]
    return lockfile
  }

  // https://github.com/pnpm/pnpm/issues/16322
  test('the current lockfile may leave out the packages of the projects the install did not select', () => {
    assertLockfilesEqual(currentLockfileOfPackageA(), wantedLockfile(), { wantedLockfileDir: '<LOCKFILE_DIR>', filteredInstall: true })
    expect(() => assertLockfilesEqual(currentLockfileOfPackageA(), wantedLockfile(), { wantedLockfileDir: '<LOCKFILE_DIR>' }))
      .toThrow('The installed dependencies in the modules directory is not up-to-date')
  })

  test('should throw if a package the current lockfile records changed', () => {
    const changedWantedLockfile = wantedLockfile()
    changedWantedLockfile.packages!['foo@1.0.0' as DepPath] = {
      resolution: { integrity: 'sha512-aaa' },
      dependencies: { bar: '1.0.0' },
    }
    expect(() => assertLockfilesEqual(currentLockfileOfPackageA(), changedWantedLockfile, { wantedLockfileDir: '<LOCKFILE_DIR>', filteredInstall: true }))
      .toThrow('The installed dependencies in the modules directory is not up-to-date')
  })

  test('should throw if an importer changed', () => {
    const changedWantedLockfile = wantedLockfile()
    changedWantedLockfile.importers['packages/a' as ProjectId].dependencies = { foo: '1.0.1' }
    expect(() => assertLockfilesEqual(currentLockfileOfPackageA(), changedWantedLockfile, { wantedLockfileDir: '<LOCKFILE_DIR>', filteredInstall: true }))
      .toThrow('The installed dependencies in the modules directory is not up-to-date')
  })
})
