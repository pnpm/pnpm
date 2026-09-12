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

function emptyLockfile (settings: Partial<LockfileObject>): LockfileObject {
  return {
    importers: {},
    lockfileVersion: '9.0',
    ...settings,
  }
}
