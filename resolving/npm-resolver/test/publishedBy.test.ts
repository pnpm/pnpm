import fs from 'fs'
import path from 'path'
import { ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { type Registries } from '@pnpm/types'
import { fixtures } from '@pnpm/test-fixtures'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import tempy from 'tempy'

const f = fixtures(__dirname)

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

/* eslint-disable @typescript-eslint/no-explicit-any */
const badDatesMeta = loadJsonFile.sync<any>(f.find('bad-dates.json'))
const isPositiveMeta = loadJsonFile.sync<any>(f.find('is-positive-full.json'))
const isPositiveAbbreviatedMeta = loadJsonFile.sync<any>(f.find('is-positive.json'))
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

  const cacheDir = tempy.directory()
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
  const cacheDir = tempy.directory()
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
  const cacheDir = tempy.directory()
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
  })).rejects.toThrow(/Version 1\.0\.0 \(released .+\) of foo does not meet the minimumReleaseAge constraint/)
})

test('should skip time field validation for excluded packages', async () => {
  const cacheDir = tempy.directory()
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

test('re-fetch full metadata when registry returns abbreviated metadata and publishedBy is set', async () => {
  // The npm registry returns abbreviated metadata by default (no per-version `time` field).
  // When publishedBy is set, pnpm needs `time` for the maturity check, so it should
  // automatically re-fetch the full metadata document.
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveAbbreviatedMeta)
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    registries,
  })
  // 3.0.0 was published 2015-07-10 (mature relative to publishedBy 2016-01-01);
  // 3.1.0 was published 2016-01-11 (not yet mature). So resolution must pick 3.0.0.
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2016-01-01T00:00:00.000Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('upgrade disk-cached abbreviated metadata to full when publishedBy is set', async () => {
  // The disk cache holds abbreviated metadata (no per-version `time`). When a
  // later install uses publishedBy, pnpm needs to upgrade to full metadata so
  // the maturity check has real `time` data.
  const cacheDir = tempy.directory()
  fs.mkdirSync(path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`), { recursive: true })
  fs.writeFileSync(
    path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org/is-positive.json`),
    JSON.stringify(isPositiveAbbreviatedMeta),
    'utf8'
  )

  // The upgrade fetch goes to the registry asking for full metadata.
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    registries,
    preferOffline: true,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {
    publishedBy: new Date('2016-01-01T00:00:00.000Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('strictPublishedByCheck=true does not rethrow ERR_PNPM_MISSING_TIME from the version-spec cache path', async () => {
  // Regression test: the version-spec fast path
  // (`!opts.updateToLatest && spec.type === 'version'`) in pickPackage used to
  // rethrow ERR_PNPM_MISSING_TIME under strictPublishedByCheck, instead of
  // falling through to the registry-fetch path. The fix lets MISSING_TIME from
  // cached abbreviated meta fall through so the fetch can upgrade to full
  // metadata and run the maturity check on real `time` data.
  const cacheDir = tempy.directory()
  fs.mkdirSync(path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org`), { recursive: true })
  // Stash abbreviated meta on disk so the version-spec fast path loads it and
  // pickPackageFromMeta throws MISSING_TIME on the maturity check.
  fs.writeFileSync(
    path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org/is-positive.json`),
    JSON.stringify(isPositiveAbbreviatedMeta),
    'utf8'
  )

  // The fall-through fetch returns full metadata with `time`.
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    registries,
    strictPublishedByCheck: true,
  })

  // Exact-version specifier hits the version-spec cache path. 3.0.0 was
  // published 2015-07-10, mature relative to publishedBy 2015-08-17.
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '3.0.0' }, {
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})
