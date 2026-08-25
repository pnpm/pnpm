import { expect, test } from '@jest/globals'
import { resolvedPackageVersionsFromLockfile } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

test('resolvedPackageVersionsFromLockfile()', () => {
  expect(resolvedPackageVersionsFromLockfile({
    importers: {},
    lockfileVersion: '9.0',
    packages: {
      ['foo@1.0.0' as DepPath]: {
        resolution: { integrity: 'AAA' },
      },
      ['foo@2.0.0(bar@1.0.0)' as DepPath]: {
        resolution: { integrity: 'BBB' },
      },
      ['@foo/bar@3.0.0' as DepPath]: {
        resolution: { integrity: 'CCC' },
      },
    },
  })).toEqual(new Map([
    ['foo', new Set(['1.0.0', '2.0.0'])],
    ['@foo/bar', new Set(['3.0.0'])],
  ]))
})

test('resolvedPackageVersionsFromLockfile() registers only the name for non-semver resolutions', () => {
  expect(resolvedPackageVersionsFromLockfile({
    importers: {},
    lockfileVersion: '9.0',
    packages: {
      ['foo@github.com/user/repo/abcdef' as DepPath]: {
        version: '1.0.0',
        resolution: { commit: 'abcdef', repo: 'https://github.com/user/repo.git', type: 'git' },
      },
    },
  })).toEqual(new Map([
    ['foo', new Set()],
  ]))
})

test('resolvedPackageVersionsFromLockfile() with no packages', () => {
  expect(resolvedPackageVersionsFromLockfile({
    importers: {},
    lockfileVersion: '9.0',
  })).toEqual(new Map())
})
