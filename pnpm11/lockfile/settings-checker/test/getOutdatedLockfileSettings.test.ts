import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'

import { getOutdatedLockfileSettings } from '../src/getOutdatedLockfileSetting.js'

test('ignoredOptionalDependencies is reported when the sets differ', () => {
  const lockfile = emptyLockfile({ ignoredOptionalDependencies: ['foo'] })

  expect(getOutdatedLockfileSettings(lockfile, { ignoredOptionalDependencies: ['bar'] }))
    .toContain('ignoredOptionalDependencies')
})

test('ignoredOptionalDependencies is not reported when the sets match in a different order', () => {
  const lockfile = emptyLockfile({ ignoredOptionalDependencies: ['foo', 'bar'] })

  expect(getOutdatedLockfileSettings(lockfile, { ignoredOptionalDependencies: ['bar', 'foo'] }))
    .not.toContain('ignoredOptionalDependencies')
})

test('the compared ignoredOptionalDependencies arrays are left in their original order', () => {
  // `createMatcher` is order-sensitive: an `!` exclusion only excludes from the
  // patterns before it, so reordering these arrays would flip which optional
  // dependencies get ignored.
  const recorded = ['*', '!foo']
  const configured = ['*', '!bar']
  const lockfile = emptyLockfile({ ignoredOptionalDependencies: recorded })

  getOutdatedLockfileSettings(lockfile, { ignoredOptionalDependencies: configured })

  expect(recorded).toStrictEqual(['*', '!foo'])
  expect(configured).toStrictEqual(['*', '!bar'])
})

test('pnpmfileChecksum is reported when it differs, unless its comparison is skipped', () => {
  const lockfile = emptyLockfile({ pnpmfileChecksum: 'sha256-abc' })

  expect(getOutdatedLockfileSettings(lockfile, {})).toContain('pnpmfileChecksum')
  expect(getOutdatedLockfileSettings(lockfile, { ignorePnpmfileChecksum: true }))
    .not.toContain('pnpmfileChecksum')
})

test('catalogs are reported when the configuration drops a recorded entry, unless their comparison is skipped', () => {
  const lockfile = emptyLockfile({
    catalogs: {
      default: {
        'is-odd': { specifier: '^3.0.1', version: '3.0.1' },
      },
    },
  })

  expect(getOutdatedLockfileSettings(lockfile, { catalogs: {} })).toContain('catalogs')
  expect(getOutdatedLockfileSettings(lockfile, { catalogs: {}, ignoreRecordedCatalogs: true }))
    .not.toContain('catalogs')
})

test('catalogs are not reported when the configuration matches the recorded entries', () => {
  const lockfile = emptyLockfile({
    catalogs: {
      default: {
        'is-odd': { specifier: '^3.0.1', version: '3.0.1' },
      },
    },
  })

  expect(getOutdatedLockfileSettings(lockfile, { catalogs: { default: { 'is-odd': '^3.0.1' } } }))
    .not.toContain('catalogs')
})

function emptyLockfile (settings: Partial<LockfileObject>): LockfileObject {
  return {
    importers: {},
    lockfileVersion: '9.0',
    ...settings,
  }
}
