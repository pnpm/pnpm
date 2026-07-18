import { expect, test } from '@jest/globals'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import { clearMeta, retainsFullMeta } from '../src/clearMeta.js'

function fullMeta (): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        libc: ['glibc'],
        scripts: { postinstall: 'node scripts/build.js' },
        description: 'dropped',
        dist: {
          tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
    time: { '1.0.0': '2020-01-01T00:00:00.000Z' },
    modified: '2020-01-01T00:00:00.000Z',
    readme: '# dropped',
  } as unknown as PackageMeta
}

test('clearMeta keeps the install-relevant field set and drops the rest', () => {
  const condensed = clearMeta(fullMeta())
  expect(condensed.versions['1.0.0'].libc).toEqual(['glibc'])
  expect(condensed.versions['1.0.0'].dist.tarball).toBe('https://registry.npmjs.org/foo/-/foo-1.0.0.tgz')
  expect(condensed.time).toEqual({ '1.0.0': '2020-01-01T00:00:00.000Z' })
  expect(condensed.modified).toBe('2020-01-01T00:00:00.000Z')
  expect(condensed.versions['1.0.0'].scripts).toBeUndefined()
  expect((condensed.versions['1.0.0'] as { description?: string }).description).toBeUndefined()
  expect((condensed as PackageMeta & { readme?: string }).readme).toBeUndefined()
})

test('clearMeta is memoized by input identity, and condensing a condensed document is the identity', () => {
  // Several layers condense the same parsed document (the settled-fetch memo
  // and the resolver cache); identity-memoization is what makes them share
  // one condensed copy instead of pinning one each.
  const meta = fullMeta()
  const condensed = clearMeta(meta)
  expect(clearMeta(meta)).toBe(condensed)
  expect(clearMeta(condensed)).toBe(condensed)
})

test('clearMeta carries the etag over so a condensed document can still answer conditional-request headers', () => {
  const withEtag = fullMeta()
  withEtag.etag = '"abc"'
  expect(clearMeta(withEtag).etag).toBe('"abc"')
  expect(clearMeta(fullMeta()).etag).toBeUndefined()
})

test('retainsFullMeta only holds for full-metadata resolvers without filterMetadata', () => {
  expect(retainsFullMeta({ fullMetadata: true })).toBe(true)
  expect(retainsFullMeta({ fullMetadata: true, filterMetadata: true })).toBe(false)
  expect(retainsFullMeta({})).toBe(false)
  expect(retainsFullMeta({ filterMetadata: true })).toBe(false)
})
