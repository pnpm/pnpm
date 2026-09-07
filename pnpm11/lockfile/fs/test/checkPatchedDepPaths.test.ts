import { expect, test } from '@jest/globals'
import { checkPatchedDepPaths } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

const CURRENT = 'aaaa1111'
const STALE = 'bbbb2222'

function lockfile (overrides: Partial<LockfileObject>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    ...overrides,
  }
}

// A git / tarball / `file:` dependency records the version the patch was matched against on
// its package entry, since its dependency path's version slot holds the reference instead.
const GIT_REF = 'git+file:///repo#0123456789012345678901234567890123456789'
const GIT_RESOLUTION = {
  type: 'git' as const,
  repo: 'file:///repo',
  commit: '0123456789012345678901234567890123456789',
}

test('checkPatchedDepPaths() accepts a lockfile whose suffixes all match patchedDependencies', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'is-positive': '1.0.0' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${CURRENT})` },
      },
    },
    packages: {
      [`is-positive@1.0.0(patch_hash=${CURRENT})` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
      },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() accepts a lockfile with no patches at all', () => {
  expect(checkPatchedDepPaths(lockfile({
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'is-positive': '1.0.0' },
        dependencies: { 'is-positive': '1.0.0' },
      },
    },
    packages: {
      ['is-positive@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() reports an importer pinned to a hash that is not in patchedDependencies', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'is-positive': '1.0.0' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${STALE})` },
      },
    },
    packages: {
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a stale snapshot key', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
      },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a stale dependency edge of a snapshot', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${STALE})` },
      },
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a suffix left behind after every patch was removed', () => {
  expect(checkPatchedDepPaths(lockfile({
    packages: {
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
      },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() looks past a peer-dependency suffix to the patch hash', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'react-dom@18.0.0': CURRENT },
    packages: {
      [`react-dom@18.0.0(patch_hash=${STALE})(react@18.0.0)` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
      },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() resolves an aliased edge to the patched package it points at', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { positive: `is-positive@1.0.0(patch_hash=${STALE})` },
      },
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() accepts a hash shared by several patched packages', () => {
  const otherHash = 'cccc3333'
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: {
      'is-positive@1.0.0': CURRENT,
      'is-odd@3.0.1': otherHash,
    },
    packages: {
      [`is-positive@1.0.0(patch_hash=${CURRENT})` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
      },
      [`is-odd@3.0.1(patch_hash=${otherHash})` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${CURRENT})` },
      },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() reports a suffix left on a hash that now belongs to a different entry', () => {
  // foo@1.0.0 and foo@2.0.0 started out sharing one patch file, so both
  // recorded SHARED. foo@2.0.0's patch has since diverged to DIVERGED, but its
  // suffix still says SHARED — a hash the map does still contain, under the
  // entry for the *other* version.
  const shared = 'aaaa1111'
  const diverged = 'dddd4444'
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: {
      'foo@1.0.0': shared,
      'foo@2.0.0': diverged,
    },
    packages: {
      [`foo@1.0.0(patch_hash=${shared})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
      [`foo@2.0.0(patch_hash=${shared})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() accepts several entries that legitimately share one patch file', () => {
  const shared = 'aaaa1111'
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: {
      'foo@1.0.0': shared,
      'foo@2.0.0': shared,
    },
    packages: {
      [`foo@1.0.0(patch_hash=${shared})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
      [`foo@2.0.0(patch_hash=${shared})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() resolves a range entry to the versions it covers', () => {
  const current = 'aaaa1111'
  const stale = 'bbbb2222'
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@^1.0.0': current },
    packages: {
      [`foo@1.5.0(patch_hash=${current})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('up-to-date')
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@^1.0.0': current },
    packages: {
      [`foo@1.5.0(patch_hash=${stale})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() matches a patched git dependency on its recorded version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.0.0': CURRENT },
    packages: {
      [`foo@${GIT_REF}(patch_hash=${CURRENT})` as DepPath]: { resolution: GIT_RESOLUTION, version: '1.0.0' },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() reports a stale hash on a patched git dependency', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.0.0': CURRENT },
    packages: {
      [`foo@${GIT_REF}(patch_hash=${STALE})` as DepPath]: { resolution: GIT_RESOLUTION, version: '1.0.0' },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() resolves a range entry against a git dependency\'s recorded version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@^1.0.0': CURRENT },
    packages: {
      [`foo@${GIT_REF}(patch_hash=${STALE})` as DepPath]: { resolution: GIT_RESOLUTION, version: '1.5.0' },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a suffix left on a git dependency that is no longer patched', () => {
  expect(checkPatchedDepPaths(lockfile({
    packages: {
      [`foo@${GIT_REF}(patch_hash=${CURRENT})` as DepPath]: { resolution: GIT_RESOLUTION, version: '1.0.0' },
    },
  }))).toBe('stale')
})

// Without a recorded version a versioned patch key cannot be matched either way, so the suffix
// cannot be judged. It is not evidence of a stale hash.
test('checkPatchedDepPaths() cannot judge a versioned patch key against a dependency with no recorded version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.0.0': CURRENT },
    packages: {
      [`foo@${GIT_REF}(patch_hash=${STALE})` as DepPath]: { resolution: GIT_RESOLUTION },
    },
  }))).toBe('indeterminate')
})

// A bare-name key matches whatever version the package turns out to have, so it is judged on the
// hash alone even with no version to recover.
test('checkPatchedDepPaths() judges a bare-name patch key without a recorded version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { foo: CURRENT },
    packages: {
      [`foo@${GIT_REF}(patch_hash=${CURRENT})` as DepPath]: { resolution: GIT_RESOLUTION },
    },
  }))).toBe('up-to-date')
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { foo: CURRENT },
    packages: {
      [`foo@${GIT_REF}(patch_hash=${STALE})` as DepPath]: { resolution: GIT_RESOLUTION },
    },
  }))).toBe('stale')
})

// A registry-qualified dependency path carries the semver behind its registry alias, and gets no
// `version` field on its package entry because of it.
test('checkPatchedDepPaths() matches a registry-qualified path on the semver behind its alias', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.1.0': CURRENT },
    packages: {
      [`foo@work:1.1.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('up-to-date')
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.1.0': CURRENT },
    packages: {
      [`foo@work:1.1.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() judges a reference to a missing entry when the path carries the version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${STALE})` },
      },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() cannot judge a reference to a missing entry whose path carries no version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { foo: `foo@${GIT_REF}(patch_hash=${STALE})` },
      },
    },
  }))).toBe('indeterminate')
})

// A definite disagreement is knowledge, and a suffix that cannot be judged elsewhere does not
// take it away.
test('checkPatchedDepPaths() reports a stale suffix even when another cannot be judged', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${STALE})` },
      },
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

// The map is the reference every suffix is judged against, so a key it cannot resolve leaves
// nothing to judge them with. The resolver reports the key itself.
test('checkPatchedDepPaths() cannot judge a lockfile whose patchedDependencies key is unparsable', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@not-a-range': CURRENT },
    packages: {
      [`foo@1.0.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('indeterminate')
})
