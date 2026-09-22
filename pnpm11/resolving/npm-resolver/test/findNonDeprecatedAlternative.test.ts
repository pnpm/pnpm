import { expect, test } from '@jest/globals'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import type { RegistryPackageSpec } from '../src/parseBareSpecifier.js'
import { findNonDeprecatedAlternative } from '../src/pickPackageFromMeta.js'

function metaWith (versions: Record<string, boolean>, time?: Record<string, string>): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': {},
    ...(time ? { time } : {}),
    versions: Object.fromEntries(
      Object.entries(versions).map(([version, deprecated]) => [
        version,
        { name: 'foo', version, ...(deprecated ? { deprecated: 'do not use' } : {}) },
      ])
    ),
  } as unknown as PackageMeta
}

function range (fetchSpec: string): RegistryPackageSpec {
  return { type: 'range', name: 'foo', fetchSpec }
}

test('picks the newest version that is not deprecated', () => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, '2.3.1': false, '2.4.0': true })
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), {})?.version).toBe('2.3.1')
})

test('reports whether reaching the alternative means widening the range', () => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, '2.3.1': true })
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), {})).toEqual({ version: '1.4.0', outsideDeclaredRange: false })
  expect(findNonDeprecatedAlternative(meta, range('^0.9.0'), {})).toEqual({ version: '1.4.0', outsideDeclaredRange: true })
})

// A tag says nothing about which versions are acceptable, so the warning must
// not claim the alternative falls outside a range the user never declared.
test('never claims a range for a tag dependency', () => {
  const meta = metaWith({ '1.0.0': true, '2.0.0': false })
  const tag: RegistryPackageSpec = { type: 'tag', name: 'foo', fetchSpec: 'latest' }
  expect(findNonDeprecatedAlternative(meta, tag, {})).toEqual({ version: '2.0.0', outsideDeclaredRange: false })
})

// Naming a version the active policy would refuse to install would send the
// user somewhere pnpm will not go.
test('skips versions the publish-date policy would refuse', () => {
  const meta = metaWith(
    { '1.0.0': true, '1.4.0': false, '2.0.0': false },
    { '1.4.0': '2020-01-01T00:00:00.000Z', '2.0.0': '2030-01-01T00:00:00.000Z' }
  )
  const opts = { publishedBy: new Date('2025-01-01T00:00:00.000Z') }
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), opts)?.version).toBe('1.4.0')
})

// `filterPkgMetadataByPublishDate` drops a version it cannot date, so naming
// one here would point at a version the pick itself would refuse.
test('skips versions the policy cannot date', () => {
  const meta = metaWith(
    { '1.0.0': true, '1.4.0': false, '2.0.0': false },
    { '1.4.0': '2020-01-01T00:00:00.000Z' } // 2.0.0 has no timestamp
  )
  const opts = { publishedBy: new Date('2025-01-01T00:00:00.000Z') }
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), opts)?.version).toBe('1.4.0')
})

// A version with no timestamp is still trusted when the policy names it.
test('keeps a version with no timestamp when the policy trusts it', () => {
  const meta = metaWith({ '1.0.0': true, '2.0.0': false }, {})
  const opts = {
    publishedBy: new Date('2025-01-01T00:00:00.000Z'),
    publishedByExclude: () => ['2.0.0'],
  }
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), opts)?.version).toBe('2.0.0')
})

// Abbreviated metadata has no `time` map at all. The pick admits every
// version once `modified` predates the cutoff, so the hint must not vanish.
test('falls back to modified when the packument carries no time map', () => {
  const meta = metaWith({ '1.0.0': true, '2.0.0': false })
  ;(meta as { modified?: string }).modified = '2020-01-01T00:00:00.000Z'
  const opts = { publishedBy: new Date('2025-01-01T00:00:00.000Z') }
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), opts)?.version).toBe('2.0.0')
})

// `modified` past the cutoff proves nothing about individual versions, and the
// pick refuses to guess, so neither does the hint.
test('names nothing when modified is past the cutoff', () => {
  const meta = metaWith({ '1.0.0': true, '2.0.0': false })
  ;(meta as { modified?: string }).modified = '2030-01-01T00:00:00.000Z'
  const opts = { publishedBy: new Date('2025-01-01T00:00:00.000Z') }
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), opts)).toBeUndefined()
})

test('is undefined when every admissible version is deprecated', () => {
  expect(findNonDeprecatedAlternative(metaWith({ '1.0.0': true, '2.0.0': true }), range('*'), {})).toBeUndefined()
})

// A registry can put anything in `versions`, and an unparseable key must not
// become the version pnpm tells the user to move to.
test('skips keys that are not versions', () => {
  const meta = metaWith({ '1.0.0': false, 'not-a-version': false })
  expect(findNonDeprecatedAlternative(meta, range('*'), {})?.version).toBe('1.0.0')
})

test.each(['2.0.0', 'v2.0.0'])('does not recommend a retry-blocked alternative under key %s', (rawKey) => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, [rawKey]: false })
  expect(findNonDeprecatedAlternative(meta, range('*'), {
    blockedVersions: new Map([['foo', new Set(['2.0.0'])]]),
  })?.version).toBe('1.4.0')
  expect(findNonDeprecatedAlternative(meta, range('*'), {
    blockedVersions: new Map([['foo', new Set(['3.0.0'])]]),
  })?.version).toBe('2.0.0')
})
