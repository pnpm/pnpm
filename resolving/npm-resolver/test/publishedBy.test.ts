import fs from 'node:fs'
import path from 'node:path'

import { ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR } from '@pnpm/constants'
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
const badDatesMeta = loadJsonFileSync<any>(f.find('bad-dates.json'))
const isPositiveMeta = loadJsonFileSync<any>(f.find('is-positive-full.json'))
const isPositiveAbbreviatedMeta = loadJsonFileSync<any>(f.find('is-positive.json'))
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

test('fall back to a newer version if there is no version published by the given date', async () => {
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/bad-dates', method: 'GET' })
    .reply(200, badDatesMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'bad-dates', bareSpecifier: '^1.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('bad-dates@1.0.0')
})

test('request metadata when the one in cache does not have a version satisfying the range', async () => {
  const cacheDir = temporaryDirectory()
  const cachedMeta = {
    'dist-tags': {},
    versions: {},
    time: {},
  }
  fs.mkdirSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(
    path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org/bad-dates.jsonl`),
    `${JSON.stringify({})}\n${JSON.stringify(cachedMeta)}`,
    'utf8'
  )

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/bad-dates', method: 'GET' })
    .reply(200, badDatesMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'bad-dates', bareSpecifier: '^1.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('bad-dates@1.0.0')
})

test('do not pick version that does not satisfy the date requirement even if it is loaded from cache and requested by exact version', async () => {
  const cacheDir = temporaryDirectory()
  const fooMeta = {
    'dist-tags': {},
    versions: {
      '1.0.0': {
        dist: {
          integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
          shasum: '857db584a1ba5d1cb2980527fc3b6c435d37b0fd',
          tarball: 'https://registry.npmjs.org/is-positive/-/foo-1.0.0.tgz',
        },
      },
    },
    time: {
      '1.0.0': '2016-08-17T19:26:00.508Z',
    },
  }
  fs.mkdirSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(
    path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org/foo.jsonl`),
    `${JSON.stringify({})}\n${JSON.stringify(fooMeta)}`,
    'utf8'
  )

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/foo', method: 'GET' })
    .reply(200, fooMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
    strictPublishedByCheck: true,
  })
  await expect(resolveFromNpm({ alias: 'foo', bareSpecifier: '1.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })).rejects.toThrow(/Version 1\.0\.0 \(released .+\) of foo does not meet the minimumReleaseAge constraint/)
})

test('should skip time field validation for excluded packages', async () => {
  const cacheDir = temporaryDirectory()
  const { time: _time, ...metaWithoutTime } = isPositiveMeta

  fs.mkdirSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org/is-positive.jsonl`), JSON.stringify(metaWithoutTime), 'utf8')

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, metaWithoutTime)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
  })

  const publishedByExclude = (pkgName: string) => pkgName === 'is-positive'

  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'latest' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
    publishedByExclude,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.manifest.version).toBe('3.1.0')
})

test('use abbreviated metadata when modified date is older than publishedBy', async () => {
  // is-positive abbreviated has modified: "2017-08-17T19:26:00.508Z"
  // publishedBy is set to 2018, so modified < publishedBy → all versions are old enough
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, isPositiveAbbreviatedMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2018-01-01T00:00:00.000Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('re-fetch full metadata when abbreviated modified date is recent', async () => {
  // Abbreviated has modified in the future relative to publishedBy → needs full metadata
  const recentAbbreviated = {
    ...isPositiveAbbreviatedMeta,
    modified: '2015-06-10T00:00:00.000Z',
  }

  const agent = getMockAgent().get(registries.default.replace(/\/$/, ''))
  // First request: abbreviated
  agent.intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, recentAbbreviated)
  // Second request: full metadata (re-fetch)
  agent.intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  // publishedBy is 2015-06-05, modified is 2015-06-10 → modified >= publishedBy → needs full
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^1.0.0' }, {
    publishedBy: new Date('2015-06-05T00:00:00.000Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  // 1.0.0 was published 2015-06-02, which is before publishedBy (2015-06-05)
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('ignoreMissingTimeField=true skips maturity check when full metadata has no time field', async () => {
  const { time: _time, ...metaWithoutTime } = isPositiveMeta

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, metaWithoutTime)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
    ignoreMissingTimeField: true,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('ignoreMissingTimeField=true still upgrades abbreviated→full when time is missing', async () => {
  // With ignoreMissingTimeField=true, pnpm should still re-fetch full metadata
  // when abbreviated metadata lacks time — only falling back to skip+warn if
  // even the full metadata has no time field. Here the full response DOES have
  // time, so the maturity check must run (and pick the old 1.0.0, not latest).
  const recentAbbreviated = {
    ...isPositiveAbbreviatedMeta,
    modified: '2015-06-10T00:00:00.000Z',
  }

  const agent = getMockAgent().get(registries.default.replace(/\/$/, ''))
  agent.intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, recentAbbreviated)
  agent.intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
    ignoreMissingTimeField: true,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^1.0.0' }, {
    publishedBy: new Date('2015-06-05T00:00:00.000Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('ignoreMissingTimeField=false throws when full metadata has no time field', async () => {
  const { time: _time, ...metaWithoutTime } = isPositiveMeta

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/is-positive', method: 'GET' })
    .reply(200, metaWithoutTime)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
    ignoreMissingTimeField: false,
  })
  await expect(resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })).rejects.toThrow(/missing the "time" field/)
})

test('ignoreMissingTimeField=true skips maturity check from disk-cached metadata lacking time', async () => {
  // Exercise the cached-metadata return path: write full metadata to disk
  // with the `time` field stripped, and verify that resolution succeeds
  // (no ERR_PNPM_MISSING_TIME) when the setting is on.
  const { time: _time, ...metaWithoutTime } = isPositiveMeta

  const cacheDir = temporaryDirectory()
  const cacheDir2 = path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(cacheDir2, { recursive: true })
  const cachePath = path.join(cacheDir2, 'is-positive.jsonl')
  fs.writeFileSync(cachePath, `${JSON.stringify({})}\n${JSON.stringify(metaWithoutTime)}`, 'utf8')

  // No mock agent intercepts — test would fail if a network request fired.

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
    ignoreMissingTimeField: true,
    offline: true,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('use cached metadata based on file mtime when publishedBy is set', async () => {
  const cacheDir = temporaryDirectory()
  // Write abbreviated metadata to the abbreviated cache dir
  const cacheDir2 = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`)
  fs.mkdirSync(cacheDir2, { recursive: true })
  const cachePath = path.join(cacheDir2, 'is-positive.jsonl')
  const headers = JSON.stringify({ modified: isPositiveAbbreviatedMeta.modified })
  fs.writeFileSync(cachePath, `${headers}\n${JSON.stringify(isPositiveAbbreviatedMeta)}`, 'utf8')

  // No mock agent intercepts — the test verifies no network request is made.
  // If a request were attempted, it would fail.

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  // publishedBy in the past relative to file mtime (file was just written = now)
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2020-01-01T00:00:00.000Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})
