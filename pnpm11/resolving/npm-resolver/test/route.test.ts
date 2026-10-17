import { describe, expect, test } from '@jest/globals'

import {
  type CacheHitCheck,
  decideCacheHitCheck,
  decideMirrorShortcuts,
  isFreshUnvalidatedMirror,
  isMirrorWithoutEtag,
  type MirrorShortcuts,
  mirrorWrittenAfter,
  readsMirrorFirst,
  type RouteInputs,
  UNVALIDATED_MIRROR_MAX_AGE_MS,
} from '../src/route.js'

const PKG_NAME = 'acme'
const CUTOFF = new Date('2026-10-01T00:00:00Z')

const RANGE_SHORTCUTS: MirrorShortcuts = {
  exactVersion: false,
  dominantLockfileVersion: true,
  unvalidatedMirror: true,
}

describe('readsMirrorFirst', () => {
  test.each<[string, RouteInputs, boolean]>([
    ['online', rangePick(), false],
    ['offline', rangePick({}, { offline: true }), true],
    ['prefer-offline', rangePick({}, { preferOffline: true }), true],
    ['pick lowest', rangePick({ pickLowestVersion: true }), true],
  ])('%s', (_name, inputs, expected) => {
    expect(readsMirrorFirst(inputs)).toBe(expected)
  })
})

describe('decideMirrorShortcuts', () => {
  test('a stable range pick may use the lockfile version and an unvalidated mirror', () => {
    expect(decideMirrorShortcuts(rangePick())).toStrictEqual(RANGE_SHORTCUTS)
  })

  test.each<[string, RouteInputs]>([
    ['latest tag', rangePick({ includeLatestTag: true })],
    ['update checksums', rangePick({ updateChecksums: true })],
    ['release age', rangePick({ publishedBy: CUTOFF })],
    ['release age with the package excluded', rangePick({ publishedBy: CUTOFF, publishedByExclude: () => true })],
    ['no-downgrade', rangePick({ trustPolicy: 'no-downgrade' })],
  ])('%s rules out the range shortcuts', (_name, inputs) => {
    expect(decideMirrorShortcuts(inputs)).toStrictEqual({ ...RANGE_SHORTCUTS, dominantLockfileVersion: false, unvalidatedMirror: false })
  })

  test('pnpm update does not reuse an unvalidated mirror', () => {
    expect(decideMirrorShortcuts(rangePick({ refreshMetadata: true }))).toStrictEqual({ ...RANGE_SHORTCUTS, unvalidatedMirror: false })
  })

  test.each<[string, RouteInputs, boolean]>([
    ['pinned version', versionPick(), true],
    ['pinned version, release age', versionPick({ publishedBy: CUTOFF }), true],
    ['pinned version, latest tag', versionPick({ includeLatestTag: true }), false],
    ['pinned version, update checksums', versionPick({ updateChecksums: true }), false],
    ['tag', { ...rangePick(), spec: { type: 'tag', name: PKG_NAME } }, false],
  ])('exact version: %s', (_name, inputs, exactVersion) => {
    const shortcuts = decideMirrorShortcuts(inputs)
    expect(shortcuts.exactVersion).toBe(exactVersion)
    expect(shortcuts.dominantLockfileVersion).toBe(false)
  })
})

describe('mirrorWrittenAfter', () => {
  test.each<[string, RouteInputs, Date | undefined]>([
    ['release age', rangePick({ publishedBy: CUTOFF }), CUTOFF],
    ['pinned version', versionPick({ publishedBy: CUTOFF }), CUTOFF],
    ['package excluded', rangePick({ publishedBy: CUTOFF, publishedByExclude: (name) => name === PKG_NAME }), undefined],
    ['another package excluded', rangePick({ publishedBy: CUTOFF, publishedByExclude: (name) => name === 'other' }), CUTOFF],
    ['no cutoff', rangePick(), undefined],
  ])('%s', (_name, inputs, expected) => {
    expect(mirrorWrittenAfter(inputs)).toStrictEqual(expected)
  })

  test('asks the exclusion only when a cutoff is set', () => {
    let asked = false
    mirrorWrittenAfter(rangePick({
      publishedByExclude: () => {
        asked = true
        return false
      },
    }))
    expect(asked).toBe(false)
  })
})

describe('isMirrorWithoutEtag', () => {
  test.each<[string, Parameters<typeof isMirrorWithoutEtag>[0], boolean]>([
    ['no etag', {}, true],
    ['empty etag', { etag: '' }, true],
    ['etag', { etag: '"etag"' }, false],
    ['uncacheable', { uncacheable: true }, false],
    ['no mirror', null, false],
  ])('%s', (_name, headers, expected) => {
    expect(isMirrorWithoutEtag(headers)).toBe(expected)
  })
})

test('an unvalidated mirror is fresh within its max age either way', () => {
  const now = CUTOFF.getTime()
  const justInside = UNVALIDATED_MIRROR_MAX_AGE_MS - 1
  expect(isFreshUnvalidatedMirror(new Date(now), now)).toBe(true)
  expect(isFreshUnvalidatedMirror(new Date(now - justInside), now)).toBe(true)
  expect(isFreshUnvalidatedMirror(new Date(now + justInside), now)).toBe(true)
  expect(isFreshUnvalidatedMirror(new Date(now - UNVALIDATED_MIRROR_MAX_AGE_MS), now)).toBe(false)
  expect(isFreshUnvalidatedMirror(new Date(now + UNVALIDATED_MIRROR_MAX_AGE_MS), now)).toBe(false)
})

describe('decideCacheHitCheck', () => {
  test.each([true, false])('a verified or offline hit is served, picked: %s', (picked) => {
    expect(decideCacheHitCheck(rangePick(), true, picked)).toBe('serve')
    expect(decideCacheHitCheck(rangePick({}, { offline: true }), false, picked)).toBe('serve')
  })

  test.each<[string, RouteInputs]>([
    ['range', rangePick()],
    ['version', versionPick()],
    ['prefer-offline', rangePick({}, { preferOffline: true })],
  ])('an unverified hit without a pick revalidates: %s', (_name, inputs) => {
    expect(decideCacheHitCheck(inputs, false, false)).toBe('revalidate')
  })

  test.each<[string, RouteInputs, CacheHitCheck]>([
    ['prefer-offline', rangePick({}, { preferOffline: true }), 'serve'],
    ['pick lowest', rangePick({ pickLowestVersion: true }), 'serve'],
    ['version', versionPick({ publishedBy: CUTOFF }), 'serve'],
    ['stable range', rangePick(), 'stableRangePick'],
    ['latest tag', rangePick({ includeLatestTag: true }), 'revalidate'],
    ['update checksums', rangePick({ updateChecksums: true }), 'revalidate'],
    ['release age', rangePick({ publishedBy: CUTOFF }), 'revalidate'],
    ['no-downgrade', rangePick({ trustPolicy: 'no-downgrade' }), 'revalidate'],
    ['tag', { ...rangePick(), spec: { type: 'tag', name: PKG_NAME } }, 'revalidate'],
  ])('an unverified hit with a pick: %s', (_name, inputs, expected) => {
    expect(decideCacheHitCheck(inputs, false, true)).toBe(expected)
  })
})

function rangePick (overrides: Partial<RouteInputs['opts']> = {}, ctx: RouteInputs['ctx'] = {}): RouteInputs {
  return { ctx, spec: { type: 'range', name: PKG_NAME }, opts: overrides }
}

function versionPick (overrides: Partial<RouteInputs['opts']> = {}, ctx: RouteInputs['ctx'] = {}): RouteInputs {
  return { ctx, spec: { type: 'version', name: PKG_NAME }, opts: overrides }
}
