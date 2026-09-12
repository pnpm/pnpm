import { expect, test } from '@jest/globals'
import type { DepPath } from '@pnpm/types'

import { resolvedPackageVersionsForPrune } from '../src/resolvedPackageVersionsForPrune.js'

const newLockfile = {
  importers: {},
  lockfileVersion: '9.0',
  packages: {
    ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'AAA' } },
  },
}

test('the versions of the freshly resolved lockfile', () => {
  expect(resolvedPackageVersionsForPrune({}, newLockfile))
    .toEqual(new Map([['foo', new Set(['1.0.0'])]]))
})

test('no versions when the lockfile is not used', () => {
  expect(resolvedPackageVersionsForPrune({
    lockfile: false,
  }, newLockfile)).toBeUndefined()
})

test('no versions when the lockfile is not shared by the whole workspace', () => {
  expect(resolvedPackageVersionsForPrune({
    sharedWorkspaceLockfile: false,
  }, newLockfile)).toBeUndefined()
})

test('no versions when no lockfile is available', () => {
  expect(resolvedPackageVersionsForPrune({}, undefined)).toBeUndefined()
})
