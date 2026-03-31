import fs from 'node:fs'
import path from 'node:path'

import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import type { Registries } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

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
  // Write cached metadata to disk
  const cacheDir2 = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(cacheDir2, { recursive: true })
  fs.writeFileSync(
    path.join(cacheDir2, 'is-positive.json'),
    JSON.stringify(isPositiveMeta),
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

test('fetch fresh metadata when registry returns 200', async () => {
  const cacheDir = temporaryDirectory()
  // Write stale cached metadata with only v1.0.0
  const staleMeta = {
    name: 'is-positive',
    versions: {
      '1.0.0': isPositiveMeta.versions['1.0.0'],
    },
    'dist-tags': { latest: '1.0.0' },
    modified: '2015-06-02T12:03:51.069Z',
  }
  const cacheDir2 = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(cacheDir2, { recursive: true })
  fs.writeFileSync(
    path.join(cacheDir2, 'is-positive.json'),
    JSON.stringify(staleMeta),
    'utf8'
  )

  // Registry returns fresh metadata
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, isPositiveMeta)

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

test('fetch without If-Modified-Since when no local cache exists', async () => {
  // No cache file, so no If-Modified-Since header — normal 200 response
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
