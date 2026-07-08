import { rmSync } from 'node:fs'
import { readFile } from 'node:fs/promises'

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

async function readMirrorWithRetry (pkgMirror: string, attempts: number): Promise<string | undefined> {
  try {
    return await readFile(pkgMirror, 'utf8')
  } catch {
    if (attempts <= 0) return undefined
    await new Promise((resolve) => setTimeout(resolve, 10))
    return readMirrorWithRetry(pkgMirror, attempts - 1)
  }
}

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
  expect(getPkgMetaCacheKey('https://reg.example.com', 'foo', false, false))
    .toBe(getPkgMetaCacheKey('https://reg.example.com/', 'foo', false, false))

  // Registries that genuinely differ by path are never collapsed.
  expect(getPkgMetaCacheKey('https://reg.example.com/team-a/', 'foo', false, false))
    .not.toBe(getPkgMetaCacheKey('https://reg.example.com/team-b/', 'foo', false, false))

  // Abbreviated and full documents keep distinct slots.
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', true, false))
    .not.toBe(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))

  // Filtered and unfiltered full metadata keep distinct slots (clearMeta
  // strips the filtered form), but filterMetadata is irrelevant to the
  // abbreviated key.
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', true, true))
    .not.toBe(getPkgMetaCacheKey(REGISTRY, 'foo', true, false))
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', false, true))
    .toBe(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))
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
  expect(ctx.metaCache.has(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))).toBe(true)

  // updateChecksums must still hit the registry, even though the warm in-memory
  // cache now holds a disk-sourced entry for this package.
  await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, updateChecksums: true, preferredVersionSelectors: undefined })
  expect(fetchedNames).toEqual(['foo'])
})

test('offline resolution promotes the disk-loaded packument into the in-memory cache', async () => {
  const meta = fooMeta()
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, undefined))

  const ctx = {
    fetch: async () => {
      throw new Error('offline resolution must not hit the network')
    },
    metaCache: createMetaCache(),
    cacheDir,
    offline: true,
  }
  // A range spec avoids the exact-version fast path, so resolution goes through
  // the offline branch that loads the packument from disk.
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }
  const opts = { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined }

  const first = await pickPackage(ctx, spec, opts)
  expect(first.pickedPackage?.version).toBe('1.0.0')
  expect(ctx.metaCache.has(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))).toBe(true)

  // Delete the on-disk mirror: a second resolve must still succeed, proving it
  // is served from the in-memory cache instead of re-reading and re-parsing disk.
  rmSync(pkgMirror)
  const second = await pickPackage(ctx, spec, opts)
  expect(second.pickedPackage?.version).toBe('1.0.0')
})

test('prefer-offline resolution promotes the disk-loaded packument into the in-memory cache', async () => {
  const meta = fooMeta()
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, undefined))

  const ctx = {
    fetch: async () => {
      throw new Error('prefer-offline resolution must not hit the network when the cache is warm')
    },
    metaCache: createMetaCache(),
    cacheDir,
    preferOffline: true,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }
  const opts = { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined }

  const first = await pickPackage(ctx, spec, opts)
  expect(first.pickedPackage?.version).toBe('1.0.0')
  expect(ctx.metaCache.has(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))).toBe(true)

  rmSync(pkgMirror)
  const second = await pickPackage(ctx, spec, opts)
  expect(second.pickedPackage?.version).toBe('1.0.0')
})

test('the raw response body is written to the disk mirror and then released from the fetch result', async () => {
  const meta = fooMeta()
  // A body distinct from the compact JSON.stringify(meta) so we can prove the
  // mirror is written from the raw response text, not re-serialized.
  const rawBody = JSON.stringify(meta, null, 2)
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')

  let fetchResult: { meta: PackageMeta, jsonText: string | undefined, etag: string | undefined } | undefined
  const ctx = {
    fetch: async () => {
      fetchResult = { meta, jsonText: rawBody, etag: undefined }
      return fetchResult
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  // A range spec avoids the exact-version fast path and, with no on-disk mirror,
  // forces the network-fetch branch that writes the mirror and caches the meta.
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  const res = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  expect(res.pickedPackage?.version).toBe('1.0.0')

  // The mirror is written fire-and-forget, so retry until it appears.
  const mirror = await readMirrorWithRetry(pkgMirror, 100)
  // The body after the headers line is the raw response text, unchanged.
  expect(mirror?.slice(mirror.indexOf('\n') + 1)).toBe(rawBody)

  // Once the mirror is written, the raw body must be released so the memoized
  // fetch result stops pinning it for the rest of the resolution phase.
  expect(fetchResult?.jsonText).toBeUndefined()
})

test('a disk-promoted cache entry that cannot satisfy the spec falls back to the registry under prefer-offline', async () => {
  const staleMeta = fooMeta()
  const freshMeta = fooMeta()
  freshMeta.versions['2.0.0'] = {
    ...staleMeta.versions['1.0.0'],
    version: '2.0.0',
    dist: {
      ...staleMeta.versions['1.0.0'].dist,
      tarball: 'https://registry.npmjs.org/foo/-/foo-2.0.0.tgz',
    },
  }
  freshMeta['dist-tags'].latest = '2.0.0'
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(staleMeta, undefined))

  const fetchedNames: string[] = []
  const ctx = {
    fetch: async (pkgName: string) => {
      fetchedNames.push(pkgName)
      return { meta: freshMeta, jsonText: JSON.stringify(freshMeta), etag: undefined }
    },
    metaCache: createMetaCache(),
    cacheDir,
    preferOffline: true,
  }
  const opts = { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined }

  // The first resolve is satisfied by the stale on-disk mirror and promotes it
  // into the in-memory cache.
  const first = await pickPackage(ctx, { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }, opts)
  expect(first.pickedPackage?.version).toBe('1.0.0')
  expect(fetchedNames).toHaveLength(0)

  // A range the promoted (registry-unverified) entry can't satisfy must fall
  // back to the registry instead of failing the pick.
  const second = await pickPackage(ctx, { type: 'range', name: 'foo', fetchSpec: '^2.0.0' }, opts)
  expect(second.pickedPackage?.version).toBe('2.0.0')
  expect(fetchedNames).toEqual(['foo'])
})
