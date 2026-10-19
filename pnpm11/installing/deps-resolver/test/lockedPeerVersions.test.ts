import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectId } from '@pnpm/types'

import { getImporterLockedPeerVersions } from '../lib/lockedPeerVersions.js'

function lockfileOf (packages: LockfileObject['packages'], dependencies: Record<string, string>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: { ['.' as ProjectId]: { specifiers: {}, dependencies } },
    packages,
  } as LockfileObject
}

test('an explicit suffix pins its providers and theirs', () => {
  const lockfile = lockfileOf({}, { a: '1.0.0(b@2.0.0(c@1.0.0))' })
  expect(getImporterLockedPeerVersions(lockfile, '.' as ProjectId)).toStrictEqual(new Map([
    ['b', new Set(['2.0.0'])],
    ['c', new Set(['1.0.0'])],
  ]))
})

test('a peer an npm alias provides is pinned under the declared name', () => {
  const lockfile = lockfileOf({
    'a@1.0.0(provider@1.0.0)': {
      resolution: { integrity: 'sha512-a' },
      dependencies: { peer: 'provider@1.0.0' },
      peerDependencies: { peer: 'npm:provider@1' },
    },
  } as unknown as LockfileObject['packages'], { a: '1.0.0(provider@1.0.0)' })
  expect(getImporterLockedPeerVersions(lockfile, '.' as ProjectId)).toStrictEqual(new Map([
    ['peer', new Set(['1.0.0'])],
  ]))
})

test('a hashed suffix pins the providers its snapshot edges name', () => {
  const lockfile = lockfileOf({
    'a@1.0.0(0123456789abcdef0123456789abcdef)': {
      resolution: { integrity: 'sha512-a' },
      dependencies: { peer: '2.0.0', other: '1.0.0' },
      peerDependencies: { peer: '^2.0.0' },
    },
  } as unknown as LockfileObject['packages'], { a: '1.0.0(0123456789abcdef0123456789abcdef)' })
  expect(getImporterLockedPeerVersions(lockfile, '.' as ProjectId)).toStrictEqual(new Map([
    ['peer', new Set(['2.0.0'])],
  ]))
})

test('a hashed suffix ignores a transitive peer that a child resolved from its own dependencies', () => {
  const lockfile = lockfileOf({
    'a@1.0.0(0123456789abcdef0123456789abcdef)': {
      resolution: { integrity: 'sha512-a' },
      dependencies: { child: '1.0.0(peer@2.0.0)', 'own-dep-child': '1.0.0' },
      transitivePeerDependencies: ['peer'],
    },
    'child@1.0.0(peer@2.0.0)': {
      resolution: { integrity: 'sha512-child' },
      dependencies: { peer: '2.0.0' },
      peerDependencies: { peer: '^2.0.0' },
    },
    'own-dep-child@1.0.0': {
      resolution: { integrity: 'sha512-own' },
      dependencies: { peer: '1.0.0' },
      peerDependencies: { peer: '^1.0.0' },
    },
  } as unknown as LockfileObject['packages'], { a: '1.0.0(0123456789abcdef0123456789abcdef)' })
  expect(getImporterLockedPeerVersions(lockfile, '.' as ProjectId)).toStrictEqual(new Map([
    ['peer', new Set(['2.0.0'])],
  ]))
})

test('a dependency named after an Object.prototype member is no peer', () => {
  const lockfile = lockfileOf({
    'a@1.0.0(0123456789abcdef0123456789abcdef)': {
      resolution: { integrity: 'sha512-a' },
      dependencies: { constructor: '1.0.0' },
      peerDependencies: {},
    },
  } as unknown as LockfileObject['packages'], { a: '1.0.0(0123456789abcdef0123456789abcdef)' })
  expect(getImporterLockedPeerVersions(lockfile, '.' as ProjectId)).toStrictEqual(new Map())
})
