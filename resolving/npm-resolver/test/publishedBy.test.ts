import fs from 'fs'
import path from 'path'
import { FULL_FILTERED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { type Registries } from '@pnpm/types'
import { fixtures } from '@pnpm/test-fixtures'
import { loadJsonFileSync } from 'load-json-file'
import nock from 'nock'
import { temporaryDirectory } from 'tempy'

const f = fixtures(import.meta.dirname)

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

/* eslint-disable @typescript-eslint/no-explicit-any */
const badDatesMeta = loadJsonFileSync<any>(f.find('bad-dates.json'))
const isPositiveMeta = loadJsonFileSync<any>(f.find('is-positive-full.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

afterEach(() => {
  nock.cleanAll()
  nock.disableNetConnect()
})

beforeEach(() => {
  nock.enableNetConnect()
})

test('fall back to a newer version if there is no version published by the given date', async () => {
  nock(registries.default)
    .get('/bad-dates')
    .reply(200, badDatesMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
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
    cachedAt: '2016-08-17T19:26:00.508Z',
  }
  fs.mkdirSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org/bad-dates.json`), JSON.stringify(cachedMeta), 'utf8')

  nock(registries.default)
    .get('/bad-dates')
    .reply(200, badDatesMeta)

  const { resolveFromNpm } = createResolveFromNpm({
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
    cachedAt: '2016-08-17T19:26:00.508Z',
  }
  fs.mkdirSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org/foo.json`), JSON.stringify(fooMeta), 'utf8')

  nock(registries.default)
    .get('/foo')
    .reply(200, fooMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registries,
    strictPublishedByCheck: true,
  })
  await expect(resolveFromNpm({ alias: 'foo', bareSpecifier: '1.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })).rejects.toThrow('No matching version found')
})

test('should skip time field validation for excluded packages', async () => {
  const cacheDir = temporaryDirectory()
  const { time: _time, ...metaWithoutTime } = isPositiveMeta

  fs.mkdirSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(path.join(cacheDir, `${FULL_FILTERED_META_DIR}/registry.npmjs.org/is-positive.json`), JSON.stringify(metaWithoutTime), 'utf8')

  nock(registries.default)
    .get('/is-positive')
    .reply(200, metaWithoutTime)

  const { resolveFromNpm } = createResolveFromNpm({
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
