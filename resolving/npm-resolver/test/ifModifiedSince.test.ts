import { closeAllMetadataCaches, MetadataCache } from '@pnpm/cache.metadata'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import type { Registries } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, registryHost, setupMockAgent, teardownMockAgent } from './utils/index.js'

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

afterEach(async () => {
  closeAllMetadataCaches()
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

const REG = registryHost(registries.default)

test('use local cache when registry returns 304 Not Modified', async () => {
  const cacheDir = temporaryDirectory()
  // Seed cached metadata with etag in SQLite
  const db = new MetadataCache(cacheDir)
  db.queueSet(`${REG}/is-positive`, JSON.stringify(isPositiveMeta), {
    etag: '"abc123"',
    modified: isPositiveMeta.modified,
    cachedAt: Date.now(),
  })
  db.flush()
  db.close()

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

  // Verify etag was saved to SQLite cache
  // The resolve function does not wait for the cache write, so retry
  const etag = await retryGetEtag(cacheDir, `${REG}/is-positive`)
  expect(etag).toBe('"xyz789"')
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

async function retryGetEtag (cacheDir: string, name: string): Promise<string | undefined> {
  let etag: string | undefined
  /* eslint-disable no-await-in-loop */
  for (let i = 0; i < 6; i++) {
    await new Promise((resolve) => setTimeout(resolve, 500))
    const db = new MetadataCache(cacheDir)
    const headers = db.getHeaders(name)
    db.close()
    if (headers?.etag) {
      etag = headers.etag
      break
    }
  }
  /* eslint-enable no-await-in-loop */
  return etag
}
