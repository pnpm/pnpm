import path from 'path'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import { type Registries } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import tempy from 'tempy'
import { retryLoadJsonFile } from './utils'

const f = fixtures(__dirname)
/* eslint-disable @typescript-eslint/no-explicit-any */
const jsrRusGreetMeta = loadJsonFile.sync<any>(f.find('jsr-rus-greet.json'))
const jsrLucaCasesMeta = loadJsonFile.sync<any>(f.find('jsr-luca-cases.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const registries = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
} satisfies Registries

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

test('resolveFromJsr() on jsr', async () => {
  const slash = '%2F'
  nock(registries.default)
    .get(`/@jsr${slash}rus__greet`)
    .reply(404)
    .get(`/@jsr${slash}luca__cases`)
    .reply(404)
  nock(registries['@jsr'])
    .get(`/@jsr${slash}rus__greet`)
    .reply(200, jsrRusGreetMeta)
    .get(`/@jsr${slash}luca__cases`)
    .reply(200, jsrLucaCasesMeta)

  const cacheDir = tempy.directory()
  const { resolveFromJsr } = createResolveFromNpm({
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
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'npm.jsr.io/@jsr/rus__greet.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta).toMatchObject({
    name: expect.any(String),
    versions: expect.any(Object),
    'dist-tags': expect.any(Object),
  })
})

test('resolveFromJsr() on jsr with alias renaming', async () => {
  const slash = '%2F'
  nock(registries.default)
    .get(`/@jsr${slash}rus__greet`)
    .reply(404)
    .get(`/@jsr${slash}luca__cases`)
    .reply(404)
  nock(registries['@jsr'])
    .get(`/@jsr${slash}rus__greet`)
    .reply(200, jsrRusGreetMeta)
    .get(`/@jsr${slash}luca__cases`)
    .reply(200, jsrLucaCasesMeta)

  const cacheDir = tempy.directory()
  const { resolveFromJsr } = createResolveFromNpm({
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
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'npm.jsr.io/@jsr/rus__greet.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta).toMatchObject({
    name: expect.any(String),
    versions: expect.any(Object),
    'dist-tags': expect.any(Object),
  })
})

test('resolveFromJsr() on jsr with packages without scope', async () => {
  const cacheDir = tempy.directory()
  const { resolveFromJsr } = createResolveFromNpm({
    cacheDir,
    registries,
  })
  await expect(resolveFromJsr({ alias: 'greet', bareSpecifier: 'jsr:0.0.3' }, {})).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
  })
})
