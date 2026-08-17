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
  expect(resolvedPackageVersionsForPrune({ minimumReleaseAgeExcludePrune: true }, newLockfile))
    .toEqual(new Map([['foo', new Set(['1.0.0'])]]))
})

test('no versions when the setting is off', () => {
  expect(resolvedPackageVersionsForPrune({}, newLockfile)).toBeUndefined()
})

test('no versions when the install resolved no lockfile', () => {
  expect(resolvedPackageVersionsForPrune({ minimumReleaseAgeExcludePrune: true }, undefined)).toBeUndefined()
})

test('no versions when the lockfile is not used', () => {
  expect(resolvedPackageVersionsForPrune({
    minimumReleaseAgeExcludePrune: true,
    lockfile: false,
  }, newLockfile)).toBeUndefined()
})

test('no versions when the lockfile is not shared by the whole workspace', () => {
  expect(resolvedPackageVersionsForPrune({
    minimumReleaseAgeExcludePrune: true,
    sharedWorkspaceLockfile: false,
  }, newLockfile)).toBeUndefined()
})
