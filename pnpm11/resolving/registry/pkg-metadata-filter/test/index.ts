import { expect, test } from '@jest/globals'
import { filterPkgMetadata } from '@pnpm/resolving.registry.pkg-metadata-filter'

test('filterPkgMetadata narrows versions and dist-tags by publish date', () => {
  const cutoff = new Date('2020-04-01T00:00:00.000Z')
  const name = 'dist-tag-date'
  expect(filterPkgMetadata({
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz`, shasum: '' },
      },
      '3.1.0': {
        name,
        version: '3.1.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.1.0.tgz`, shasum: '' },
        deprecated: 'This version is deprecated',
      },
      '3.2.0': {
        name,
        version: '3.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.2.0.tgz`, shasum: '' },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz`, shasum: '' },
      },
    },
    'dist-tags': {
      latest: '3.2.0',
    },
    time: {
      '2.9.9': '2020-01-01T00:00:00.000Z',
      '3.0.0': '2020-02-01T00:00:00.000Z',
      '3.1.0': '2020-03-01T00:00:00.000Z',
      '3.2.0': '2020-05-01T00:00:00.000Z',
    },
  }, { publishedBy: cutoff })).toMatchSnapshot()

  expect(filterPkgMetadata({
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz`, shasum: '' },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz`, shasum: '' },
      },
    },
    'dist-tags': {
      latest: '3.0.0',
      stable: '3.0.0',
    },
    time: {
      '2.9.9': '2020-03-01T00:00:00.000Z',
      '3.0.0': '2020-05-01T00:00:00.000Z',
    },
  }, { publishedBy: cutoff })).toMatchSnapshot()
})

test('latest fallback does not exceed the original dist-tag target', () => {
  const cutoff = new Date('2026-07-15T00:00:00.000Z')
  const name = 'latest-fallback'
  const packageVersion = (version: string) => ({
    name,
    version,
    dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`, shasum: '' },
  })

  const pkgDoc = {
    name,
    versions: {
      '3.0.0': packageVersion('3.0.0'),
      '3.0.1': packageVersion('3.0.1'),
      '4.0.0': packageVersion('4.0.0'),
    },
    'dist-tags': {
      latest: '3.0.1',
    },
    time: {
      '3.0.0': '2026-07-01T00:00:00.000Z',
      '3.0.1': '2026-07-15T12:00:00.000Z',
      '4.0.0': '2025-10-10T00:00:00.000Z',
    },
  }

  const filtered = filterPkgMetadata(pkgDoc, { publishedBy: cutoff })

  expect(filtered['dist-tags'].latest).toBe('3.0.0')

  const withoutSafeFallback = filterPkgMetadata({
    ...pkgDoc,
    versions: {
      '3.0.1': packageVersion('3.0.1'),
      '4.0.0': packageVersion('4.0.0'),
    },
  }, { publishedBy: cutoff })
  expect(withoutSafeFallback['dist-tags'].latest).toBeUndefined()
})

test('custom dist-tag fallback does not exceed the original target', () => {
  const cutoff = new Date('2026-07-25T00:00:00.000Z')
  const name = 'nightly-fallback'
  const packageVersion = (version: string) => ({
    name,
    version,
    dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`, shasum: '' },
  })

  const filtered = filterPkgMetadata({
    name,
    versions: {
      '0.0.29-nightly.20260724.896': packageVersion('0.0.29-nightly.20260724.896'),
      '0.0.29-nightly.20260725.899': packageVersion('0.0.29-nightly.20260725.899'),
      '0.1.0-alpha.1': packageVersion('0.1.0-alpha.1'),
    },
    'dist-tags': {
      nightly: '0.0.29-nightly.20260725.899',
    },
    time: {
      '0.0.29-nightly.20260724.896': '2026-07-24T20:37:59.752Z',
      '0.0.29-nightly.20260725.899': '2026-07-25T04:18:17.590Z',
      '0.1.0-alpha.1': '2026-02-28T23:12:56.014Z',
    },
  }, { publishedBy: cutoff })

  expect(filtered['dist-tags'].nightly).toBe('0.0.29-nightly.20260724.896')
})

test('filtering is memoized per packument and the per-packument policy cache stays bounded', () => {
  const name = 'memoized-pkg'
  const doc = {
    name,
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-1.0.0.tgz`, shasum: '' },
      },
    },
    'dist-tags': {
      latest: '1.0.0',
    },
    time: {
      '1.0.0': '2020-01-01T00:00:00.000Z',
    },
  }
  const cutoff = new Date('2020-04-01T00:00:00.000Z')

  const first = filterPkgMetadata(doc, { publishedBy: cutoff })
  expect(filterPkgMetadata(doc, { publishedBy: cutoff })).toBe(first)
  // The key is the cutoff's value, not the Date object's identity.
  expect(filterPkgMetadata(doc, { publishedBy: new Date(cutoff.getTime()) })).toBe(first)
  // A different cutoff gets its own slot.
  expect(filterPkgMetadata(doc, { publishedBy: new Date('2020-06-01T00:00:00.000Z') })).not.toBe(first)

  // Exceeding the per-packument cap with distinct cutoffs evicts the oldest
  // entry instead of growing forever: the original cutoff is recomputed.
  for (let i = 1; i <= 4; i++) {
    filterPkgMetadata(doc, { publishedBy: new Date(cutoff.getTime() + i * 60_000) })
  }
  expect(filterPkgMetadata(doc, { publishedBy: cutoff })).not.toBe(first)
})

test('a version or dist-tag named __proto__ stays an own key of the filtered metadata', () => {
  // Parsed from JSON, the way a registry response is: an object literal would
  // apply `__proto__` as the prototype instead of keeping it as a key.
  const doc = JSON.parse(`{
    "name": "proto-pkg",
    "versions": {
      "__proto__": {
        "name": "proto-pkg",
        "version": "1.0.0",
        "polluted": true,
        "dist": { "tarball": "https://registry.npmjs.org/proto-pkg/-/proto-pkg-1.0.0.tgz", "shasum": "" }
      },
      "1.0.0": {
        "name": "proto-pkg",
        "version": "1.0.0",
        "dist": { "tarball": "https://registry.npmjs.org/proto-pkg/-/proto-pkg-1.0.0.tgz", "shasum": "" }
      }
    },
    "dist-tags": { "latest": "1.0.0", "__proto__": "1.0.0" },
    "time": { "__proto__": "2020-01-01T00:00:00.000Z", "1.0.0": "2020-01-01T00:00:00.000Z" }
  }`)

  const filtered = filterPkgMetadata(doc, { publishedBy: new Date('2020-04-01T00:00:00.000Z') })

  expect(Object.keys(filtered.versions)).toContain('__proto__')
  expect(Object.keys(filtered['dist-tags'])).toContain('__proto__')
  // Assigning `__proto__` to a plain object would have made the malicious
  // version manifest the prototype of the map, exposing its fields as
  // versions.
  expect('polluted' in filtered.versions).toBe(false)
})

test('blocked versions are dropped and dist-tags fall back past them', () => {
  const name = 'blocked-pkg'
  const packageVersion = (version: string) => ({
    name,
    version,
    dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`, shasum: '' },
  })
  const doc = {
    name,
    versions: {
      '1.0.0': packageVersion('1.0.0'),
      '1.1.0': packageVersion('1.1.0'),
      '1.2.0': packageVersion('1.2.0'),
    },
    'dist-tags': {
      latest: '1.2.0',
    },
    time: {
      '1.0.0': '2020-01-01T00:00:00.000Z',
      '1.1.0': '2020-02-01T00:00:00.000Z',
      '1.2.0': '2020-03-01T00:00:00.000Z',
    },
  }

  const filtered = filterPkgMetadata(doc, {
    publishedBy: new Date('2020-04-01T00:00:00.000Z'),
    blockedVersions: new Set(['1.2.0']),
  })

  expect(Object.keys(filtered.versions).sort()).toStrictEqual(['1.0.0', '1.1.0'])
  expect(filtered['dist-tags'].latest).toBe('1.1.0')
})

test('blocked versions are dropped with no publish-date cutoff and without a time field', () => {
  // The shape a package covered wholesale by `minimumReleaseAgeExclude` takes:
  // the cutoff does not apply to it, but a version whose own dependencies
  // cannot satisfy the cutoff still has to go.
  const name = 'excluded-pkg'
  const packageVersion = (version: string) => ({
    name,
    version,
    dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`, shasum: '' },
  })

  const filtered = filterPkgMetadata({
    name,
    versions: {
      '2.0.0': packageVersion('2.0.0'),
      '2.1.0': packageVersion('2.1.0'),
    },
    'dist-tags': {
      latest: '2.1.0',
    },
  }, { blockedVersions: new Set(['2.1.0']) })

  expect(Object.keys(filtered.versions)).toStrictEqual(['2.0.0'])
  expect(filtered['dist-tags'].latest).toBe('2.0.0')
})

test('blocked versions get their own memoization slot', () => {
  const name = 'memoized-blocked'
  const doc = {
    name,
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-1.0.0.tgz`, shasum: '' },
      },
    },
    'dist-tags': { latest: '1.0.0' },
    time: { '1.0.0': '2020-01-01T00:00:00.000Z' },
  }
  const publishedBy = new Date('2020-04-01T00:00:00.000Z')

  const unblocked = filterPkgMetadata(doc, { publishedBy })
  expect(filterPkgMetadata(doc, { publishedBy })).toBe(unblocked)
  const blocked = filterPkgMetadata(doc, { publishedBy, blockedVersions: new Set(['1.0.0']) })
  expect(blocked).not.toBe(unblocked)
  expect(Object.keys(blocked.versions)).toStrictEqual([])
})
