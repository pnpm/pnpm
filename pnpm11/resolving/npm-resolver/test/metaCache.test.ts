import { rmSync } from 'node:fs'
import { readFile } from 'node:fs/promises'

import { expect, jest, test } from '@jest/globals'
import { ABBREVIATED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { temporaryDirectory } from 'tempy'

import type { FetchMetadataOptions } from '../src/fetch.js'
import { memoizeFetchMetadata } from '../src/memoizeFetchMetadata.js'
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

test('the raw response body is written verbatim to the disk mirror', async () => {
  const meta = fooMeta()
  // A body distinct from the compact JSON.stringify(meta) so we can prove the
  // mirror is written from the raw response text, not re-serialized.
  const rawBody = JSON.stringify(meta, null, 2)
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')

  const ctx = {
    fetch: async () => ({ meta, jsonText: rawBody, etag: undefined }),
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
})

test('projects sharing one in-flight fetch mirror the fetched body instead of re-serializing it', async () => {
  const meta = fooMeta()
  const rawBody = JSON.stringify(meta)
  const cacheDir = temporaryDirectory()
  const projects = 20

  let fetches = 0
  let joined = 0
  let allJoined!: () => void
  const inFlight = new Promise<void>((resolve) => {
    allJoined = resolve
  })
  const memoized = memoizeFetchMetadata(async () => {
    fetches++
    // Hold the request open until every project has joined it, so the fan-out
    // this guards against is reproduced rather than raced for.
    await inFlight
    return { meta, jsonText: rawBody, etag: undefined }
  })
  const ctx = {
    fetch: async (pkgName: string, opts: FetchMetadataOptions) => {
      if (++joined === projects) allJoined()
      return memoized.fetch(pkgName, opts)
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  const stringifySpy = jest.spyOn(JSON, 'stringify')
  try {
    const picks = await Promise.all(Array.from({ length: projects }, async () =>
      pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
    ))
    expect(picks.every((pick) => pick.pickedPackage?.version === '1.0.0')).toBe(true)
    expect(fetches).toBe(1)
    // Re-serializing per project is what exhausted the heap: the body reaches
    // tens of MB for a popular package, and every project holds its own copy
    // until the mirror write limiter drains.
    const serializations = stringifySpy.mock.calls.filter(([value]) => value === meta).length
    expect(serializations).toBe(0)
  } finally {
    stringifySpy.mockRestore()
  }
})

test('a full document fetched for an optional dependency is condensed in memory while the mirror keeps the raw body', async () => {
  const meta = fooMeta()
  meta.versions['1.0.0'].libc = ['glibc']
  meta.versions['1.0.0'].scripts = { postinstall: 'node scripts/build.js' }
  ;(meta as PackageMeta & { readme: string }).readme = '# a readme the size of a novel'
  meta.time = { '1.0.0': '2020-01-01T00:00:00.000Z' }
  const rawBody = JSON.stringify(meta)
  const cacheDir = temporaryDirectory()

  const ctx = {
    fetch: async () => ({ meta, jsonText: rawBody, etag: undefined }),
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  const res = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, optional: true, preferredVersionSelectors: undefined })
  expect(res.pickedPackage?.libc).toEqual(['glibc'])
  expect(res.meta.time).toEqual({ '1.0.0': '2020-01-01T00:00:00.000Z' })
  expect(res.pickedPackage?.scripts).toBeUndefined()
  expect((res.meta as PackageMeta & { readme?: string }).readme).toBeUndefined()
  expect(ctx.metaCache.get(getPkgMetaCacheKey(REGISTRY, 'foo', true, false))).toBe(res.meta)

  // The full-metadata mirror still receives the raw response body.
  const pkgMirror = getPkgMirrorPath(cacheDir, FULL_META_DIR, REGISTRY, 'foo')
  const mirror = await readMirrorWithRetry(pkgMirror, 100)
  expect(mirror?.slice(mirror.indexOf('\n') + 1)).toBe(rawBody)
})

test('a mirror holding a full document is condensed when promoted into the in-memory cache', async () => {
  const meta = fooMeta()
  meta.versions['1.0.0'].scripts = { postinstall: 'node scripts/build.js' }
  ;(meta as PackageMeta & { readme: string }).readme = '# a readme the size of a novel'
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
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  const res = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  expect(res.pickedPackage?.version).toBe('1.0.0')
  expect(res.pickedPackage?.scripts).toBeUndefined()
  const cached = ctx.metaCache.get(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))
  expect(cached).toBe(res.meta)
  expect((cached as PackageMeta & { readme?: string }).readme).toBeUndefined()
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

test('pickPackage retries once without validators when a 304 loses its cache body', async () => {
  const meta = fooMeta()
  meta.versions['1.0.0'].scripts = { postinstall: 'echo cache-race-marker' }
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, '"stale"'))

  type CacheBypassFetchMetadataOptions = FetchMetadataOptions & { cacheBypass?: boolean }
  const fetchCalls: CacheBypassFetchMetadataOptions[] = []
  const ctx = {
    fetch: async (_pkgName: string, opts: FetchMetadataOptions) => {
      fetchCalls.push(opts)
      if (fetchCalls.length === 1) {
        rmSync(pkgMirror)
        return { notModified: true as const }
      }
      return { meta, jsonText: JSON.stringify(meta), etag: '"fresh"' }
    },
    metaCache: createMetaCache(),
    cacheDir,
    filterMetadata: true,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  const result = await pickPackage(ctx, spec, {
    registry: REGISTRY,
    dryRun: false,
    preferredVersionSelectors: undefined,
  })

  expect(result.pickedPackage?.version).toBe('1.0.0')
  expect(fetchCalls).toHaveLength(2)
  expect(fetchCalls[0]).toMatchObject({ etag: '"stale"' })
  expect(fetchCalls[1]).toMatchObject({ cacheBypass: true })
  expect(fetchCalls[1].etag).toBeUndefined()
  expect(fetchCalls[1].modified).toBeUndefined()
  expect(result.meta.etag).toBe('"fresh"')
  expect(result.meta.versions['1.0.0'].scripts).toBeUndefined()
  expect(ctx.metaCache.get(getPkgMetaCacheKey(REGISTRY, 'foo', false, true))).toBe(result.meta)

  const mirror = await readMirrorWithRetry(pkgMirror, 100)
  expect(mirror).toBeDefined()
  if (mirror == null) throw new Error('fresh mirror was not persisted')
  const newlineIdx = mirror.indexOf('\n')
  const persistedHeaders = JSON.parse(mirror.slice(0, newlineIdx))
  const persistedMeta = JSON.parse(mirror.slice(newlineIdx + 1))
  expect(persistedHeaders.etag).toBe('"fresh"')
  expect(persistedMeta.versions['1.0.0'].scripts).toBeUndefined()
})

test('pickPackage stops after one cache-loss fallback', async () => {
  const meta = fooMeta()
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, '"stale"'))

  type CacheBypassFetchMetadataOptions = FetchMetadataOptions & { cacheBypass?: boolean }
  const fetchCalls: CacheBypassFetchMetadataOptions[] = []
  const ctx = {
    fetch: async (_pkgName: string, opts: FetchMetadataOptions) => {
      fetchCalls.push(opts)
      if (fetchCalls.length === 1) rmSync(pkgMirror)
      return { notModified: true as const }
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  await expect(pickPackage(ctx, spec, {
    registry: REGISTRY,
    dryRun: false,
    preferredVersionSelectors: undefined,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE' })
  expect(fetchCalls).toHaveLength(2)
  expect(fetchCalls[1]).toMatchObject({ cacheBypass: true })
  expect(fetchCalls[1].etag).toBeUndefined()
  expect(fetchCalls[1].modified).toBeUndefined()
})

test('pickPackage propagates a cache-loss fallback error', async () => {
  const meta = fooMeta()
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, '"stale"'))

  type CacheBypassFetchMetadataOptions = FetchMetadataOptions & { cacheBypass?: boolean }
  const fetchCalls: CacheBypassFetchMetadataOptions[] = []
  const fallbackError = new Error('fallback failed')
  const ctx = {
    fetch: async (_pkgName: string, opts: FetchMetadataOptions) => {
      fetchCalls.push(opts)
      if (fetchCalls.length === 1) {
        rmSync(pkgMirror)
        return { notModified: true as const }
      }
      throw fallbackError
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  await expect(pickPackage(ctx, spec, {
    registry: REGISTRY,
    dryRun: false,
    preferredVersionSelectors: undefined,
  })).rejects.toBe(fallbackError)
  expect(fetchCalls).toHaveLength(2)
  expect(fetchCalls[1]).toMatchObject({ cacheBypass: true })
  expect(fetchCalls[1].etag).toBeUndefined()
  expect(fetchCalls[1].modified).toBeUndefined()
})
