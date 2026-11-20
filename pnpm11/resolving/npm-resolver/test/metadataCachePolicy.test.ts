import { describe, expect, test } from '@jest/globals'

import {
  conditionalRequest,
  type DocumentKind,
  documentKindOf,
  type MetadataRequest,
  mirrorMayAnswer,
  mirrorWrite,
  releaseAgeUpgradeRequest,
  unconditionalRefetch,
} from '../src/metadataCachePolicy.js'
import type { MetaHeaders } from '../src/mirrorLayout.js'

const MODIFIED = '2026-10-01T00:00:00.000Z'
const ABBREVIATED: MetaHeaders = { etag: '"abbreviated"', modified: MODIFIED }
const FULL_IN_ABBREVIATED: MetaHeaders = { fullEtag: '"full"', modified: MODIFIED }

describe('documentKindOf', () => {
  test.each<[string, boolean, MetaHeaders | null, DocumentKind]>([
    ['full mirror', true, FULL_IN_ABBREVIATED, 'full'],
    ['abbreviated mirror', false, ABBREVIATED, 'abbreviated'],
    ['no mirror', false, null, 'abbreviated'],
    ['upgraded abbreviated mirror', false, FULL_IN_ABBREVIATED, 'fullInAbbreviatedMirror'],
    ['empty fullEtag', false, { fullEtag: '' }, 'abbreviated'],
  ])('%s', (_name, mirrorFullMetadata, headers, kind) => {
    expect(documentKindOf(mirrorFullMetadata, headers)).toBe(kind)
  })
})

describe('conditionalRequest', () => {
  test.each<[string, boolean, MetaHeaders | null, MetadataRequest]>([
    ['no mirror', false, null, { fullMetadata: false, etag: undefined, modified: undefined, cacheBypass: false }],
    ['abbreviated mirror', false, ABBREVIATED, { fullMetadata: false, etag: '"abbreviated"', modified: MODIFIED, cacheBypass: false }],
    ['full mirror', true, { etag: '"full"', modified: MODIFIED }, { fullMetadata: true, etag: '"full"', modified: MODIFIED, cacheBypass: false }],
    [
      'upgraded abbreviated mirror sends the full document\'s tag',
      false,
      FULL_IN_ABBREVIATED,
      { fullMetadata: true, etag: '"full"', modified: MODIFIED, cacheBypass: false },
    ],
    [
      'uncacheable mirror bypasses caches',
      false,
      { ...ABBREVIATED, uncacheable: true },
      { fullMetadata: false, etag: '"abbreviated"', modified: MODIFIED, cacheBypass: true },
    ],
  ])('%s', (_name, mirrorFullMetadata, headers, request) => {
    expect(conditionalRequest(mirrorFullMetadata, headers)).toStrictEqual(request)
  })
})

test('an unconditional refetch sends no validators and bypasses caches', () => {
  expect(unconditionalRefetch(false)).toStrictEqual({ fullMetadata: false, cacheBypass: true })
  expect(unconditionalRefetch(true)).toStrictEqual({ fullMetadata: true, cacheBypass: true })
})

describe('releaseAgeUpgradeRequest', () => {
  test.each<[string, MetaHeaders | null, MetadataRequest]>([
    ['an abbreviated document sends no validators', ABBREVIATED, { fullMetadata: true, cacheBypass: false }],
    [
      'an uncacheable abbreviated document sends no validators',
      { ...ABBREVIATED, uncacheable: true },
      { fullMetadata: true, cacheBypass: false },
    ],
    ['no mirror', null, { fullMetadata: true, cacheBypass: false }],
    [
      'a held full document is revalidated with its tag',
      FULL_IN_ABBREVIATED,
      { fullMetadata: true, etag: '"full"', modified: MODIFIED, cacheBypass: false },
    ],
    [
      'an uncacheable held full document bypasses caches',
      { ...FULL_IN_ABBREVIATED, uncacheable: true },
      { fullMetadata: true, etag: '"full"', modified: MODIFIED, cacheBypass: true },
    ],
  ])('%s', (_name, headers, request) => {
    expect(releaseAgeUpgradeRequest(headers)).toStrictEqual(request)
  })
})

describe('mirrorWrite', () => {
  test.each<[string, { etag?: string, fullMetadata?: boolean, uncacheable?: boolean }, boolean, ReturnType<typeof mirrorWrite>]>([
    [
      'abbreviated response, abbreviated mirror',
      { etag: '"a"' },
      false,
      { kind: 'abbreviated', etag: '"a"', body: { uncacheable: undefined, fullEtag: undefined } },
    ],
    [
      'full response, full mirror',
      { etag: '"f"', fullMetadata: true },
      true,
      { kind: 'full', etag: '"f"', body: { uncacheable: undefined, fullEtag: undefined } },
    ],
    [
      'full response, abbreviated mirror keeps the tag as fullEtag',
      { etag: '"f"', fullMetadata: true },
      false,
      { kind: 'fullInAbbreviatedMirror', etag: undefined, body: { uncacheable: undefined, fullEtag: '"f"' } },
    ],
    [
      'uncacheable response',
      { etag: '"a"', uncacheable: true },
      false,
      { kind: 'abbreviated', etag: '"a"', body: { uncacheable: true, fullEtag: undefined } },
    ],
  ])('%s', (_name, response, mirrorFullMetadata, write) => {
    expect(mirrorWrite(response, mirrorFullMetadata)).toStrictEqual(write)
  })

  test('a written mirror revalidates the document it recorded', () => {
    const write = mirrorWrite({ etag: '"f"', fullMetadata: true }, false)
    const headers = { etag: write.etag, ...write.body, modified: MODIFIED }
    expect(documentKindOf(false, headers)).toBe(write.kind)
    expect(conditionalRequest(false, headers).etag).toBe('"f"')
  })
})

describe('mirrorMayAnswer', () => {
  test.each<[string, { offline?: boolean, preferOffline?: boolean }, boolean | undefined, boolean]>([
    ['online, cacheable', {}, undefined, true],
    ['online, uncacheable', {}, true, false],
    ['prefer-offline, uncacheable', { preferOffline: true }, true, true],
    ['offline, uncacheable', { offline: true }, true, true],
  ])('%s', (_name, ctx, uncacheable, expected) => {
    expect(mirrorMayAnswer(ctx, uncacheable)).toBe(expected)
  })
})
