import fs from 'fs'
import path from 'path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import _createResolveFromNpm from '@pnpm/npm-resolver'
import fixtures from '@pnpm/test-fixtures'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import tempy from 'tempy'

const f = fixtures(__dirname)
const registry = 'https://registry.npmjs.org/'

/* eslint-disable @typescript-eslint/no-explicit-any */
const badDatesMeta = loadJsonFile.sync<any>(f.find('bad-dates.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const fetch = createFetchFromRegistry({})
const getCredentials = () => ({ authHeaderValue: undefined, alwaysAuth: undefined })
const createResolveFromNpm = _createResolveFromNpm.bind(null, fetch, getCredentials)

test('fall back to a newer version if there is no version published by the given date', async () => {
  nock(registry)
    .get('/bad-dates')
    .reply(200, badDatesMeta)

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
  })
  const resolveResult = await resolve({ alias: 'bad-dates', pref: '^1.0.0' }, {
    registry,
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('registry.npmjs.org/bad-dates/1.0.0')
})

test('request metadata when the one in cache does not have a version satisfiyng the range', async () => {
  const cacheDir = tempy.directory()
  const cachedMeta = {
    'dist-tags': {},
    versions: {},
    time: {},
    cachedAt: '2016-08-17T19:26:00.508Z',
  }
  fs.mkdirSync(path.join(cacheDir, 'metadata-v1.1/registry.npmjs.org'), { recursive: true })
  fs.writeFileSync(path.join(cacheDir, 'metadata-v1.1/registry.npmjs.org/bad-dates.json'), JSON.stringify(cachedMeta), 'utf8')

  nock(registry)
    .get('/bad-dates')
    .reply(200, badDatesMeta)

  const resolve = createResolveFromNpm({
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
  })
  const resolveResult = await resolve({ alias: 'bad-dates', pref: '^1.0.0' }, {
    registry,
    publishedBy: new Date('2015-08-17T19:26:00.508Z'),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('registry.npmjs.org/bad-dates/1.0.0')
})
