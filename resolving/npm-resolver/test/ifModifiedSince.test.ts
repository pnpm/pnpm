import fs from 'node:fs'
import path from 'node:path'

import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import type { Registries } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

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

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

test('use local cache when registry returns 304 Not Modified', async () => {
  const cacheDir = temporaryDirectory()
  // Write cached metadata with etag to disk
  // is-positive.json already has modified: "2017-08-17T19:26:00.508Z"
  const cachedMeta = {
    ...isPositiveMeta,
    etag: '"abc123"',
  }
  const cacheDir2 = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(cacheDir2, { recursive: true })
  fs.writeFileSync(
    path.join(cacheDir2, 'is-positive.json'),
    JSON.stringify(cachedMeta),
    'utf8'
  )

  // Registry returns 304 Not Modified
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
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

test('store etag and lastModified from 200 response in cache', async () => {
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

  // Verify etag and lastModified were saved to disk cache
  const cachePath = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org/is-positive.json`)
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
