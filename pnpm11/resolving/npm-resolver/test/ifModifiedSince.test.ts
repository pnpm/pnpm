import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR, FULL_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import type { Registries } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import {
  fetchAbbreviatedMetadataCached,
  fetchFullMetadataCached,
} from '../src/fetchFullMetadataCached.js'
import { getPkgMirrorPath, prepareJsonForDisk, saveMeta } from '../src/pickPackage.js'
import { getMockAgent, retryLoadJsonFile, setupMockAgent, teardownMockAgent } from './utils/index.js'

const f = fixtures(import.meta.dirname)

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

/* eslint-disable @typescript-eslint/no-explicit-any */
const isPositiveMeta = loadJsonFileSync<any>(f.find('is-positive.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

type CachedMetadataFetcher = typeof fetchFullMetadataCached
type CachedMetadata = Awaited<ReturnType<CachedMetadataFetcher>>

const cachedMetadataCases = [
  {
    fetchMetadata: fetchFullMetadataCached,
    kind: 'full',
    metaDir: FULL_META_DIR,
    stripsScripts: false,
  },
  {
    fetchMetadata: fetchAbbreviatedMetadataCached,
    kind: 'abbreviated',
    metaDir: ABBREVIATED_META_DIR,
    stripsScripts: true,
  },
] satisfies ReadonlyArray<{
  fetchMetadata: CachedMetadataFetcher
  kind: 'full' | 'abbreviated'
  metaDir: string
  stripsScripts: boolean
}>

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

test('use local cache when registry returns 304 Not Modified', async () => {
  const cacheDir = temporaryDirectory()
  // Write cached metadata with etag to disk in NDJSON format:
  // Line 1: cache headers, Line 2: registry metadata
  const cacheDir2 = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(cacheDir2, { recursive: true })
  const headers = JSON.stringify({ etag: '"abc123"', modified: isPositiveMeta.modified })
  fs.writeFileSync(
    path.join(cacheDir2, 'is-positive.jsonl'),
    `${headers}\n${JSON.stringify(isPositiveMeta)}`,
    'utf8'
  )

  // Registry returns 304 Not Modified — verify conditional headers are sent
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: {
        'if-none-match': '"abc123"',
      },
    })
    .reply(304, '')

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm(
    { alias: 'is-positive', bareSpecifier: '^3.0.0' },
    {}
  )

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('a 304 Not Modified renews the metadata file mtime so the publishedBy freshness shortcut can fire again', async () => {
  const cacheDir = temporaryDirectory()
  const metaDir = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(metaDir, { recursive: true })
  const metaPath = path.join(metaDir, 'is-positive.jsonl')
  const headers = JSON.stringify({ etag: '"abc123"', modified: isPositiveMeta.modified })
  fs.writeFileSync(metaPath, `${headers}\n${JSON.stringify(isPositiveMeta)}`, 'utf8')
  // Age the mirror far past any maturity cutoff.
  const aged = new Date(Date.now() - 365 * 24 * 60 * 60 * 1000)
  fs.utimesSync(metaPath, aged, aged)

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: {
        'if-none-match': '"abc123"',
      },
    })
    .reply(304, '')

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm(
    { alias: 'is-positive', bareSpecifier: '^3.0.0' },
    {}
  )
  expect(resolveResult!.id).toBe('is-positive@3.1.0')

  // The touch is fire-and-forget, so poll briefly instead of asserting
  // immediately.
  const renewed = () => fs.statSync(metaPath).mtime.getTime() > aged.getTime() + 1000
  await new Promise<void>((resolve) => {
    const start = Date.now()
    const timer = setInterval(() => {
      if (renewed() || Date.now() - start > 5000) {
        clearInterval(timer)
        resolve()
      }
    }, 50)
  })
  expect(renewed()).toBe(true)
})

test('store etag from 200 response in cache', async () => {
  const cacheDir = temporaryDirectory()
  const responseHeaders = {
    etag: '"xyz789"',
  }

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, isPositiveMeta, { headers: responseHeaders })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm(
    { alias: 'is-positive', bareSpecifier: '^3.0.0' },
    {}
  )

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')

  // Verify etag was saved to disk cache
  const cachePath = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org/is-positive.jsonl`)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const savedMeta = await retryLoadJsonFile<any>(cachePath)
  expect(savedMeta.etag).toBe('"xyz789"')
})

test('fetch without conditional headers when no local cache exists', async () => {
  // No cache file → no ETag/Last-Modified to send → normal 200 response
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm(
    { alias: 'is-positive', bareSpecifier: '^3.0.0' },
    {}
  )

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('retry when an unconditional metadata request receives 304 Not Modified', async () => {
  const registry = getMockAgent().get(registries.default.replace(/\/$/, ''))
  registry
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(304, '')
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: {
        'cache-control': 'no-cache',
      },
    })
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm(
    { alias: 'is-positive', bareSpecifier: '^3.0.0' },
    {}
  )

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('report an invalid response when an unconditional 304 retry also returns 304', async () => {
  const registry = getMockAgent().get(registries.default.replace(/\/$/, ''))
  registry
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(304, '')
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: {
        'cache-control': 'no-cache',
      },
    })
    .reply(304, '')

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })

  await expect(resolveFromNpm(
    { alias: 'is-positive', bareSpecifier: '^3.0.0' },
    {}
  )).rejects.toMatchObject({
    code: 'ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE',
    message: 'Registry returned 304 for is-positive without an existing cache to refresh.',
  })
})

test.each(cachedMetadataCases)('cached $kind metadata retries once without validators when the body disappears after 304', async ({
  fetchMetadata,
  metaDir,
  stripsScripts,
}) => {
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, metaDir, registries.default, 'is-positive')
  await saveMeta(pkgMirror, prepareJsonForDisk(isPositiveMeta, '"stale"'))
  const responseMeta = structuredClone(isPositiveMeta)
  responseMeta.versions['3.1.0'].scripts = { postinstall: 'echo cache-race-marker' }

  const registry = getMockAgent().get(registries.default.replace(/\/$/, ''))
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: { 'if-none-match': '"stale"' },
    })
    .reply(() => {
      fs.rmSync(pkgMirror)
      return { statusCode: 304, data: '' }
    })
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: matchCacheBypassHeaders,
    })
    .reply(200, responseMeta, {
      headers: {
        etag: '"fresh"',
        'content-type': 'application/json',
      },
    })

  const result = await fetchMetadata({
    fetch,
    retry: { retries: 0 },
    timeout: 30_000,
    fetchWarnTimeoutMs: 30_000,
  }, 'is-positive', {
    cacheDir,
    registry: registries.default,
  })

  expect(result.name).toBe('is-positive')
  expect(result.versions['3.1.0'].scripts == null).toBe(stripsScripts)
  const persisted = await retryLoadJsonFile<CachedMetadata>(pkgMirror)
  expect(persisted.etag).toBe('"fresh"')
  expect(persisted.name).toBe('is-positive')
  expect(persisted.versions['3.1.0'].scripts == null).toBe(stripsScripts)
  getMockAgent().assertNoPendingInterceptors()
})

test('cached metadata stops after one cache-loss fallback', async () => {
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, FULL_META_DIR, registries.default, 'is-positive')
  await saveMeta(pkgMirror, prepareJsonForDisk(isPositiveMeta, '"stale"'))

  const registry = getMockAgent().get(registries.default.replace(/\/$/, ''))
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: { 'if-none-match': '"stale"' },
    })
    .reply(() => {
      fs.rmSync(pkgMirror)
      return { statusCode: 304, data: '' }
    })
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: matchCacheBypassHeaders,
    })
    .reply(304, '')

  await expect(fetchFullMetadataCached({
    fetch,
    retry: { retries: 0 },
    timeout: 30_000,
    fetchWarnTimeoutMs: 30_000,
  }, 'is-positive', {
    cacheDir,
    registry: registries.default,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE' })
  getMockAgent().assertNoPendingInterceptors()
})

test('cached metadata keeps body retries in cache-bypass mode', async () => {
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, FULL_META_DIR, registries.default, 'is-positive')
  await saveMeta(pkgMirror, prepareJsonForDisk(isPositiveMeta, '"stale"'))

  const registry = getMockAgent().get(registries.default.replace(/\/$/, ''))
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: { 'if-none-match': '"stale"' },
    })
    .reply(() => {
      fs.rmSync(pkgMirror)
      return { statusCode: 304, data: '' }
    })
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: matchCacheBypassHeaders,
    })
    .reply(200, '{')
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: matchCacheBypassHeaders,
    })
    .reply(200, isPositiveMeta, {
      headers: { etag: '"after-retry"' },
    })

  const result = await fetchFullMetadataCached({
    fetch,
    retry: { retries: 1, factor: 1, minTimeout: 1, maxTimeout: 1 },
    timeout: 30_000,
    fetchWarnTimeoutMs: 30_000,
  }, 'is-positive', {
    cacheDir,
    registry: registries.default,
  })

  expect(result.name).toBe('is-positive')
  getMockAgent().assertNoPendingInterceptors()
  const persisted = await retryLoadJsonFile<CachedMetadata>(pkgMirror)
  expect(persisted.etag).toBe('"after-retry"')
})

test('cached metadata propagates a cache-loss fallback registry error', async () => {
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, FULL_META_DIR, registries.default, 'is-positive')
  await saveMeta(pkgMirror, prepareJsonForDisk(isPositiveMeta, '"stale"'))

  const registry = getMockAgent().get(registries.default.replace(/\/$/, ''))
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: { 'if-none-match': '"stale"' },
    })
    .reply(() => {
      fs.rmSync(pkgMirror)
      return { statusCode: 304, data: '' }
    })
  registry
    .intercept({
      path: '/is-positive',
      method: 'GET',
      headers: matchCacheBypassHeaders,
    })
    .reply(403, '')

  await expect(fetchFullMetadataCached({
    fetch,
    retry: { retries: 0 },
    timeout: 30_000,
    fetchWarnTimeoutMs: 30_000,
  }, 'is-positive', {
    cacheDir,
    registry: registries.default,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_FETCH_403',
    response: { status: 403 },
  })
  getMockAgent().assertNoPendingInterceptors()
})

function matchCacheBypassHeaders (headers: Record<string, string>): boolean {
  return headers['if-none-match'] === undefined &&
    headers['if-modified-since'] === undefined &&
    headers['cache-control'] === 'no-cache'
}
