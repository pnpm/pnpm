import { expect, test } from '@jest/globals'
import { checkPatchedDepPaths } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

const CURRENT = 'aaaa1111'
const STALE = 'bbbb2222'

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
// take it away. Importers are walked first, so the suffix that cannot be judged is met first.
test('checkPatchedDepPaths() reports a stale suffix even when another cannot be judged', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.0.0': CURRENT, 'is-positive@1.0.0': CURRENT },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { foo: GIT_REF },
        dependencies: { foo: `${GIT_REF}(patch_hash=${STALE})` },
      },
    },
    packages: {
      [`is-positive@1.0.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

// A key that does not resolve leaves its own package with nothing to be judged against. The
// resolver reports the key itself.
test('checkPatchedDepPaths() cannot judge a package whose patchedDependencies key is unparsable', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@not-a-range': CURRENT },
    packages: {
      [`foo@1.0.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('indeterminate')
})

test('checkPatchedDepPaths() still judges other packages when one patchedDependencies key is unparsable', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@not-a-range': CURRENT, 'bar@1.0.0': CURRENT },
    packages: {
      [`foo@1.0.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
      [`bar@1.0.0(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a snapshot key missing the suffix its patch calls for', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-positive@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports an importer reference missing the suffix its patch calls for', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'is-positive': '1.0.0' },
        dependencies: { 'is-positive': '1.0.0' },
      },
    },
    packages: {
      [`is-positive@1.0.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a dependency edge missing the suffix its patch calls for', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { positive: 'is-positive@1.0.0' },
      },
      [`is-positive@1.0.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() reports a patched git dependency missing its suffix', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'foo@1.0.0': CURRENT },
    packages: {
      [`foo@${GIT_REF}` as DepPath]: { resolution: GIT_RESOLUTION, version: '1.0.0' },
    },
  }))).toBe('stale')
})

test('checkPatchedDepPaths() accepts a version without a suffix of a package whose patch covers another version', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { 'is-positive': '2.0.0' },
        dependencies: { 'is-positive': '2.0.0' },
      },
    },
    packages: {
      ['is-positive@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() cannot judge a patch-hash marker that is not a complete suffix', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'is-positive@1.0.0': CURRENT },
    packages: {
      ['is-odd@3.0.1' as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { 'is-positive': `1.0.0(patch_hash=${STALE}` },
      },
    },
  }))).toBe('indeterminate')
})

test('checkPatchedDepPaths() cannot judge a patch-hash marker placed after the peers', () => {
  expect(checkPatchedDepPaths(lockfile({
    packages: {
      [`foo@1.0.0(react@18.0.0)(patch_hash=${STALE})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('indeterminate')
})

// A peer's segment is that peer's own dependency path, so a patched peer carries its hash inside
// the peer segment of every package that sees it.
test('checkPatchedDepPaths() accepts a patched peer nested in a peer segment', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'react@18.0.0': CURRENT },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { foo: '1.0.0', react: '18.0.0' },
        dependencies: {
          foo: `1.0.0(react@18.0.0(patch_hash=${CURRENT}))`,
          react: `18.0.0(patch_hash=${CURRENT})`,
        },
      },
    },
    packages: {
      [`foo@1.0.0(react@18.0.0(patch_hash=${CURRENT}))` as DepPath]: {
        resolution: { integrity: 'sha512-fake' },
        dependencies: { react: `18.0.0(patch_hash=${CURRENT})` },
      },
      [`react@18.0.0(patch_hash=${CURRENT})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('up-to-date')
})

test('checkPatchedDepPaths() reports a stale hash nested in a peer segment', () => {
  expect(checkPatchedDepPaths(lockfile({
    patchedDependencies: { 'react@18.0.0': CURRENT },
    packages: {
      [`foo@1.0.0(react@18.0.0(patch_hash=${STALE}))` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('stale')
})

// An unmatched `)` must not let later parentheses rebalance the suffix and hide a marker the
// leading segment does not hold.
test('checkPatchedDepPaths() cannot judge a marker behind an unmatched parenthesis', () => {
  for (const depPath of [`foo@1.0.0)(patch_hash=${STALE}`, `foo@1.0.0(peer@1.0.0))(patch_hash=${STALE})((x)`]) {
    expect(checkPatchedDepPaths(lockfile({
      packages: {
        [depPath as DepPath]: { resolution: { integrity: 'sha512-fake' } },
      },
    }))).toBe('indeterminate')
  }
})

// Nesting depth alone is not a defect: a peer many levels down is judged like any other.
test('checkPatchedDepPaths() judges a patched peer nested many levels deep', () => {
  for (const [hash, expected] of [[CURRENT, 'up-to-date'], [STALE, 'stale']] as const) {
    let depPath = `react@18.0.0(patch_hash=${hash})`
    for (let level = 0; level < 64; level++) {
      depPath = `p${level}@1.0.0(${depPath})`
    }
    expect(checkPatchedDepPaths(lockfile({
      patchedDependencies: { 'react@18.0.0': CURRENT },
      packages: {
        [depPath as DepPath]: { resolution: { integrity: 'sha512-fake' } },
      },
    }))).toBe(expected)
  }
})

// Text between segments would hide a marker from `parse`, so the suffix has to be segments back
// to back.
test('checkPatchedDepPaths() cannot judge text between suffix segments', () => {
  expect(checkPatchedDepPaths(lockfile({
    packages: {
      [`foo@1.0.0(patch_hash=${STALE})junk(peer@1.0.0)` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('indeterminate')
})

test('checkPatchedDepPaths() cannot judge a nested peer segment that is not a dependency path', () => {
  expect(checkPatchedDepPaths(lockfile({
    packages: {
      [`a@1.0.0(b@1.0.0(patch_hash=${STALE})junk)` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
    },
  }))).toBe('indeterminate')
})

// A locator can hold parentheses of its own, so the suffix is read from the end, as `parse` reads
// it.
test('checkPatchedDepPaths() does not read parentheses in a file locator as suffix segments', () => {
  for (const locator of ['file:../pkg(foo).tgz', 'file:..\\pkg(foo).tgz']) {
    for (const [hash, expected] of [[CURRENT, 'up-to-date'], [STALE, 'stale']] as const) {
      expect(checkPatchedDepPaths(lockfile({
        patchedDependencies: { foo: CURRENT },
        packages: {
          [`foo@${locator}(patch_hash=${hash})` as DepPath]: { resolution: { integrity: 'sha512-fake' } },
        },
      }))).toBe(expected)
    }
  }
})

function lockfile (overrides: Partial<LockfileObject>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    ...overrides,
  }
}
