import { expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { temporaryDirectory } from 'tempy'

import type { RegistryPackageSpec } from '../src/parseBareSpecifier.js'
import {
  getPkgMetaCacheKey,
  getPkgMirrorPath,
  type PackageMetaCache,
  pickPackage,
  prepareJsonForDisk,
  saveMeta,
} from '../src/pickPackage.js'

const REGISTRY = 'https://registry.npmjs.org/'

function fooMeta (): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        dist: {
          tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
  } as unknown as PackageMeta
}

function createMetaCache (): PackageMetaCache {
  const store = new Map<string, PackageMeta>()
  return {
    get: (key) => store.get(key),
    set: (key, meta) => {
      store.set(key, meta)
    },
    has: (key) => store.has(key),
  }
}

test('getPkgMetaCacheKey canonicalizes the registry so trailing-slash variants share one key', () => {
  // A configured named registry without a trailing slash and the verifier's
  // trailing-slashed prefix for the same registry must map to one cache slot.
  expect(getPkgMetaCacheKey('https://reg.example.com', 'foo', false))
    .toBe(getPkgMetaCacheKey('https://reg.example.com/', 'foo', false))

  // Registries that genuinely differ by path are never collapsed.
  expect(getPkgMetaCacheKey('https://reg.example.com/team-a/', 'foo', false))
    .not.toBe(getPkgMetaCacheKey('https://reg.example.com/team-b/', 'foo', false))

  // Abbreviated and full documents keep distinct slots.
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', true))
    .not.toBe(getPkgMetaCacheKey(REGISTRY, 'foo', false))
})

test('updateChecksums bypasses the in-memory cache so a disk-promoted entry cannot skip revalidation', async () => {
  const meta = fooMeta()
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, undefined))

  const fetchedNames: string[] = []
  const ctx = {
    fetch: async (pkgName: string) => {
      fetchedNames.push(pkgName)
      return { meta, jsonText: JSON.stringify(meta), etag: undefined }
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'version', name: 'foo', fetchSpec: '1.0.0' }

  // A normal resolve takes the on-disk exact-version fast path: no network, and
  // it promotes the disk-loaded packument into the in-memory cache.
  const first = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  expect(first.pickedPackage?.version).toBe('1.0.0')
  expect(fetchedNames).toHaveLength(0)
  expect(ctx.metaCache.has(getPkgMetaCacheKey(REGISTRY, 'foo', false))).toBe(true)

  // updateChecksums must still hit the registry, even though the warm in-memory
  // cache now holds a disk-sourced entry for this package.
  await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, updateChecksums: true, preferredVersionSelectors: undefined })
  expect(fetchedNames).toEqual(['foo'])
})
