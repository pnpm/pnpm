import { expect, test } from '@jest/globals'
import { filterPkgMetadataByPublishDate } from '@pnpm/resolving.registry.pkg-metadata-filter'

test('filterPkgMetadataByPublishDate', () => {
  const cutoff = new Date('2020-04-01T00:00:00.000Z')
  const name = 'dist-tag-date'
  expect(filterPkgMetadataByPublishDate({
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
  }, cutoff)).toMatchSnapshot()

  expect(filterPkgMetadataByPublishDate({
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
  }, cutoff)).toMatchSnapshot()
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

  const filtered = filterPkgMetadataByPublishDate(pkgDoc, cutoff)

  expect(filtered['dist-tags'].latest).toBe('3.0.0')

  const withoutSafeFallback = filterPkgMetadataByPublishDate({
    ...pkgDoc,
    versions: {
      '3.0.1': packageVersion('3.0.1'),
      '4.0.0': packageVersion('4.0.0'),
    },
  }, cutoff)
  expect(withoutSafeFallback['dist-tags'].latest).toBeUndefined()
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

  const first = filterPkgMetadataByPublishDate(doc, cutoff)
  expect(filterPkgMetadataByPublishDate(doc, cutoff)).toBe(first)
  // The key is the cutoff's value, not the Date object's identity.
  expect(filterPkgMetadataByPublishDate(doc, new Date(cutoff.getTime()))).toBe(first)
  // A different cutoff gets its own slot.
  expect(filterPkgMetadataByPublishDate(doc, new Date('2020-06-01T00:00:00.000Z'))).not.toBe(first)

  // Exceeding the per-packument cap with distinct cutoffs evicts the oldest
  // entry instead of growing forever: the original cutoff is recomputed.
  for (let i = 1; i <= 4; i++) {
    filterPkgMetadataByPublishDate(doc, new Date(cutoff.getTime() + i * 60_000))
  }
  expect(filterPkgMetadataByPublishDate(doc, cutoff)).not.toBe(first)
})
