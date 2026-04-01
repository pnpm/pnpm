import { closeAllMetadataCaches } from '@pnpm/cache.metadata'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import type { Registries } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, retryLoadFromCache, setupMockAgent, teardownMockAgent } from './utils/index.js'

const f = fixtures(import.meta.dirname)
/* eslint-disable @typescript-eslint/no-explicit-any */
const jsrRusGreetMeta = loadJsonFileSync<any>(f.find('jsr-rus-greet.json'))
const jsrLucaCasesMeta = loadJsonFileSync<any>(f.find('jsr-luca-cases.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const registries = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
} satisfies Registries

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

test('resolveFromJsr() on jsr', async () => {
  const slash = '%2F'
  const defaultPool = getMockAgent().get(registries.default.replace(/\/$/, ''))
  defaultPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(404, {})
  defaultPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(404, {})
  const jsrPool = getMockAgent().get(registries['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)
  jsrPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(200, jsrLucaCasesMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromJsr({ alias: '@rus/greet', bareSpecifier: 'jsr:0.0.3' }, { calcSpecifier: true })

  expect(resolveResult).toMatchObject({
    resolvedVia: 'jsr-registry',
    id: '@jsr/rus__greet@0.0.3',
    latest: '0.0.3',
    manifest: {
      name: '@jsr/rus__greet',
      version: '0.0.3',
    },
    resolution: {
      integrity: expect.any(String),
      tarball: 'https://npm.jsr.io/~/11/@jsr/rus__greet/0.0.3.tgz',
    },
    normalizedBareSpecifier: 'jsr:0.0.3',
  })

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadFromCache(cacheDir, '@jsr/rus__greet', 'abbreviated', 'https://npm.jsr.io/')
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolveFromJsr() on jsr with alias renaming', async () => {
  const slash = '%2F'
  const defaultPool = getMockAgent().get(registries.default.replace(/\/$/, ''))
  defaultPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(404, {})
  defaultPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(404, {})
  const jsrPool = getMockAgent().get(registries['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)
  jsrPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(200, jsrLucaCasesMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromJsr({ alias: 'greet', bareSpecifier: 'jsr:@rus/greet@0.0.3' }, {})

  expect(resolveResult).toMatchObject({
    resolvedVia: 'jsr-registry',
    id: '@jsr/rus__greet@0.0.3',
    latest: '0.0.3',
    manifest: {
      name: '@jsr/rus__greet',
      version: '0.0.3',
    },
    resolution: {
      integrity: expect.any(String),
      tarball: 'https://npm.jsr.io/~/11/@jsr/rus__greet/0.0.3.tgz',
    },
  })

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadFromCache(cacheDir, '@jsr/rus__greet', 'abbreviated', 'https://npm.jsr.io/')
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolveFromJsr() on jsr with packages without scope', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  await expect(resolveFromJsr({ alias: 'greet', bareSpecifier: 'jsr:0.0.3' }, {})).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
  })
})
