import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import {
  createNpmResolver,
} from '@pnpm/resolving.npm-resolver'
import { EXISTING_VERSION_SELECTOR_WEIGHT } from '@pnpm/resolving.resolver-base'
import { fixtures } from '@pnpm/test-fixtures'
import type { RegistriesByScope } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { getPkgMirrorPath, prepareJsonForDisk, saveMeta } from '../src/pickPackage.js'
import { getMockAgent, retryLoadJsonFile, setupMockAgent, teardownMockAgent } from './utils/index.js'

const f = fixtures(import.meta.dirname)
/* eslint-disable @typescript-eslint/no-explicit-any */
const jsrRusGreetMeta = loadJsonFileSync<any>(f.find('jsr-rus-greet.json'))
const jsrLucaCasesMeta = loadJsonFileSync<any>(f.find('jsr-luca-cases.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const registriesByScope = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
} satisfies RegistriesByScope

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

test('resolveFromJsr() on jsr', async () => {
  const slash = '%2F'
  const defaultPool = getMockAgent().get(registriesByScope.default.replace(/\/$/, ''))
  defaultPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(404, {})
  defaultPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(404, {})
  const jsrPool = getMockAgent().get(registriesByScope['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)
  jsrPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(200, jsrLucaCasesMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
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
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'npm.jsr.io/@jsr/rus__greet.jsonl')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta).toMatchObject({
    name: expect.any(String),
    versions: expect.any(Object),
    'dist-tags': expect.any(Object),
  })
})

test('resolveFromJsr() on jsr with alias renaming', async () => {
  const slash = '%2F'
  const defaultPool = getMockAgent().get(registriesByScope.default.replace(/\/$/, ''))
  defaultPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(404, {})
  defaultPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(404, {})
  const jsrPool = getMockAgent().get(registriesByScope['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)
  jsrPool.intercept({ path: `/@jsr${slash}luca__cases`, method: 'GET' }).reply(200, jsrLucaCasesMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
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
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'npm.jsr.io/@jsr/rus__greet.jsonl')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta).toMatchObject({
    name: expect.any(String),
    versions: expect.any(Object),
    'dist-tags': expect.any(Object),
  })
})

test('resolveFromJsr() revalidates cached ranges under trust downgrade protection', async () => {
  const slash = '%2F'
  const jsrPool = getMockAgent().get(registriesByScope['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)
  const cacheDir = temporaryDirectory()
  await saveMeta(
    getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, registriesByScope['@jsr'], '@jsr/rus__greet'),
    prepareJsonForDisk(jsrRusGreetMeta, undefined)
  )
  const fetchedUrls: string[] = []
  const countingFetch: typeof fetch = async (url, opts) => {
    fetchedUrls.push(url.toString())
    return fetch(url, opts)
  }
  const { resolveFromJsr } = createNpmResolver(countingFetch, getAuthHeader, {
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
  })

  await resolveFromJsr(
    { alias: '@rus/greet', bareSpecifier: 'jsr:^0.0.1' },
    {
      preferredVersions: {
        '@jsr/rus__greet': {
          '0.0.3': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
        },
      },
      trustPolicy: 'no-downgrade',
    }
  )

  expect(fetchedUrls).toEqual(['https://npm.jsr.io/@jsr%2Frus__greet'])
})

test('resolveFromJsr() on jsr with packages without scope', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
  })
  await expect(resolveFromJsr({ alias: 'greet', bareSpecifier: 'jsr:0.0.3' }, {})).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
  })
})

test('resolveFromJsr() returns the immature pick with policyViolation when publishedBy excludes it', async () => {
  // jsr-rus-greet's 0.0.3 was published 2024-11-16; passing a `publishedBy`
  // before that makes the version immature relative to the cutoff. The
  // resolver always falls back to the requested version and flags the
  // result with `policyViolation`; the install command (or other caller)
  // decides what to do with it. This is the named-registry / jsr path's
  // coverage for inline violation reporting.
  const slash = '%2F'
  const defaultPool = getMockAgent().get(registriesByScope.default.replace(/\/$/, ''))
  defaultPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(404, {})
  const jsrPool = getMockAgent().get(registriesByScope['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
  })
  const result = await resolveFromJsr(
    { alias: '@rus/greet', bareSpecifier: 'jsr:0.0.3' },
    {
      publishedBy: new Date('2020-01-01T00:00:00Z'),
    }
  )

  expect(result).toMatchObject({
    id: '@jsr/rus__greet@0.0.3',
    policyViolation: {
      name: '@jsr/rus__greet',
      version: '0.0.3',
      code: 'MINIMUM_RELEASE_AGE_VIOLATION',
    },
  })
})

test('resolveFromJsr() suppresses latest when publishedBy holds back the raw tag', async () => {
  // jsr-rus-greet has 0.0.1 (16:08), 0.0.2 (16:13), 0.0.3 (16:31); dist-tags.latest = 0.0.3.
  // publishedBy between 0.0.2 and 0.0.3 leaves 0.0.3 immature, so the JSR path
  // (pickFromSimpleRegistry) must suppress latest rather than surface a tag the
  // policy would refuse to install.
  const slash = '%2F'
  const defaultPool = getMockAgent().get(registriesByScope.default.replace(/\/$/, ''))
  defaultPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(404, {})
  const jsrPool = getMockAgent().get(registriesByScope['@jsr'].replace(/\/$/, ''))
  jsrPool.intercept({ path: `/@jsr${slash}rus__greet`, method: 'GET' }).reply(200, jsrRusGreetMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromJsr } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registriesByScope,
  })
  const result = await resolveFromJsr(
    { alias: '@rus/greet', bareSpecifier: 'jsr:0.0.2' },
    { publishedBy: new Date('2024-11-16T16:20:00.000Z') }
  )

  expect(result).toMatchObject({
    resolvedVia: 'jsr-registry',
    id: '@jsr/rus__greet@0.0.2',
  })
  expect(result!.latest).toBeUndefined()
})
