import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
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
