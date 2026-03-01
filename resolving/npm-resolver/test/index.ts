/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import {
  createNpmResolver,
  RegistryResponseError,
  NoMatchingVersionError,
} from '@pnpm/npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import { type Registries, type ProjectRootDir } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import nock from 'nock'
import { omit } from 'ramda'
import { temporaryDirectory } from 'tempy'
import { delay, retryLoadMsgpackFile } from './utils/index.js'

const f = fixtures(import.meta.dirname)
/* eslint-disable @typescript-eslint/no-explicit-any */
const isPositiveMeta = loadJsonFileSync<any>(f.find('is-positive.json'))
const isPositiveMetaWithDeprecated = loadJsonFileSync<any>(f.find('is-positive-with-deprecated.json'))
const isPositiveMetaFull = loadJsonFileSync<any>(f.find('is-positive-full.json'))
const isPositiveBrokenMeta = loadJsonFileSync<any>(f.find('is-positive-broken.json'))
const sindresorhusIsMeta = loadJsonFileSync<any>(f.find('sindresorhus-is.json'))
const jsonMeta = loadJsonFileSync<any>(f.find('JSON.json'))
const brokenIntegrity = loadJsonFileSync<any>(f.find('broken-integrity.json'))
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

test('resolveFromNpm()', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, { calcSpecifier: true })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.normalizedBareSpecifier).toBe('1.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadMsgpackFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'registry.npmjs.org/is-positive.mpk')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolveFromNpm() strips port 80 from http tarball URLs', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      versions: {
        '1.0.0': {
          ...isPositiveMeta.versions['1.0.0'],
          dist: {
            ...isPositiveMeta.versions['1.0.0'].dist,
            tarball: 'http://registry.npmjs.org:80/is-positive/-/is-positive-1.0.0.tgz',
          },
        },
      },
    })

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, { calcSpecifier: true })

  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'http://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
})

test('resolveFromNpm() does not save mutated meta to the cache', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})

  resolveResult!.manifest!.version = '1000'

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadMsgpackFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'registry.npmjs.org/is-positive.mpk')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.versions['1.0.0'].version).toBe('1.0.0')
})

test('resolveFromNpm() should save metadata to a unique file when the package name has upper case letters', async () => {
  nock(registries.default)
    .get('/JSON')
    .reply(200, jsonMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'JSON', bareSpecifier: '1.0.0' }, {})

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('JSON@1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadMsgpackFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, `registry.npmjs.org/JSON_${createHexHash('JSON')}.mpk`)) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('relative workspace protocol is skipped', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ bareSpecifier: 'workspace:../is-positive' }, {
    projectDir: '/home/istvan/src',
  })

  expect(resolveResult).toBeNull()
})

test('dry run', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    dryRun: true,
  })

  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  await delay(500)
  expect(fs.existsSync(path.join(cacheDir, resolveResult!.id, '..', 'index.json'))).toBeFalsy()
})

test('resolve to latest when no bareSpecifier specified', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive' }, {})
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('resolve to defaultTag when no bareSpecifier specified', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive' }, {
    defaultTag: 'stable',
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('resolve to biggest non-deprecated version that satisfies the range', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMetaWithDeprecated)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '3' }, {
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('resolve to a deprecated version if there are no non-deprecated ones that satisfy the range', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMetaWithDeprecated)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '2' }, {})
  expect(resolveResult!.id).toBe('is-positive@2.0.0')
})

test('can resolve aliased dependency', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', bareSpecifier: 'npm:is-positive@1.0.0' }, {})
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('can resolve aliased dependency w/o version specifier', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', bareSpecifier: 'npm:is-positive' }, {})
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('can resolve aliased dependency w/o version specifier to default tag', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', bareSpecifier: 'npm:is-positive' }, {
    defaultTag: 'stable',
    calcSpecifier: true,
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
  expect(resolveResult!.normalizedBareSpecifier).toBe('npm:is-positive@^3.0.0')
})

test('can resolve aliased scoped dependency', async () => {
  nock(registries.default)
    .get('/@sindresorhus%2Fis')
    .reply(200, sindresorhusIsMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is', bareSpecifier: 'npm:@sindresorhus/is@0.6.0' }, {})
  expect(resolveResult!.id).toBe('@sindresorhus/is@0.6.0')
})

test('can resolve aliased scoped dependency w/o version specifier', async () => {
  nock(registries.default)
    .get('/@sindresorhus%2Fis')
    .reply(200, sindresorhusIsMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is', bareSpecifier: 'npm:@sindresorhus/is' }, {})
  expect(resolveResult!.id).toBe('@sindresorhus/is@0.7.0')
})

test('can resolve package with version prefixed with v', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'v1.0.0' }, {})
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('can resolve package version loosely', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '= 1.0.0' }, {})
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test("resolves to latest if it's inside the wanted range. Even if there are newer versions available inside the range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {})

  // 3.1.0 is available but latest is 3.0.0, so preferring it
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("resolves to latest if it's inside the preferred range. Even if there are newer versions available inside the preferred range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^3.0.0': 'range' },
    },
  })

  // 3.1.0 is available but latest is 3.0.0, so preferring it
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("resolve using the wanted range, when it doesn't intersect with the preferred range. Even if the preferred range contains the latest version", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '2.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^2.0.0': 'range' },
    },
  })

  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("use the preferred version if it's inside the wanted range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '3.0.0': 'version' },
    },
  })

  // 3.1.0 is the latest but we prefer the 3.0.0
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("ignore the preferred version if it's not inside the wanted range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '2.0.0': 'version' },
    },
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('use the preferred range if it intersects with the wanted range', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '1.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '>=1.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^3.0.0': 'range' },
    },
  })

  // 1.0.0 is the latest but we prefer a version that is also in the preferred range
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('use the preferred range if it intersects with the wanted range (an array of preferred versions is passed)', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '1.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '>=1.0.0',
  }, {
    preferredVersions: {
      'is-positive': {
        '3.0.0': 'version',
        '3.1.0': 'version',
      },
    },
  })

  // 1.0.0 is the latest but we prefer a version that is also in the preferred range
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("ignore the preferred range if it doesn't intersect with the wanted range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^2.0.0': 'range' },
    },
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("use the preferred dist-tag if it's inside the wanted range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.1.0',
        stable: '3.0.0',
      },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { stable: 'tag' },
    },
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("ignore the preferred dist-tag if it's not inside the wanted range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.1.0',
        stable: '2.0.0',
      },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { stable: 'tag' },
    },
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("prefer a version that is both inside the wanted and preferred ranges. Even if it's not the latest of any of them", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.0.0',
      },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '1.0.0 || 2.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '1.0.0 || 3.0.0': 'range' },
    },
  })
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('prefer the version that is matched by more preferred selectors', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^3.0.0': 'range', '3.0.0': 'version' },
    },
  })

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('prefer the version that has bigger weight in preferred selectors', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': {
        '^3.0.0': 'range',
        '3.0.0': { selectorType: 'version', weight: 100 },
        '3.1.0': 'version',
      },
    },
  })

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('offline resolution fails when package meta not found in the store', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    offline: true,
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  await expect(resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})).rejects
    .toThrow(
      new PnpmError('NO_OFFLINE_META', `Failed to resolve is-positive@1.0.0 in package mirror ${path.join(cacheDir, ABBREVIATED_META_DIR, 'registry.npmjs.org/is-positive.mpk')}`)
    )
})

test('offline resolution succeeds when package meta is found in the store', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()

  {
    const { resolveFromNpm } = createResolveFromNpm({
      offline: false,
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })

    // This request will save the package's meta in the store
    await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
  }

  {
    const { resolveFromNpm } = createResolveFromNpm({
      offline: true,
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })

    const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
    expect(resolveResult!.id).toBe('is-positive@1.0.0')
  }
})

test('prefer offline resolution does not fail when package meta not found in the store', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    preferOffline: true,
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })

  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const cacheDir = temporaryDirectory()

  {
    const { resolveFromNpm } = createResolveFromNpm({
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })

    // This request will save the package's meta in the store
    await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
  }

  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  {
    const { resolveFromNpm } = createResolveFromNpm({
      preferOffline: true,
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })

    const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {})
    expect(resolveResult!.id).toBe('is-positive@3.0.0')
  }

  nock.cleanAll()
})

test('error is thrown when package is not found in the registry', async () => {
  const notExistingPackage = 'foo'

  nock(registries.default)
    .get(`/${notExistingPackage}`)
    .reply(404, {})

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  await expect(resolveFromNpm({ alias: notExistingPackage, bareSpecifier: '1.0.0' }, {})).rejects
    .toThrow(
      new RegistryResponseError(
        {
          url: `${registries.default}${notExistingPackage}`,
        },
        {
          status: 404,
          // statusText: 'Not Found',
          statusText: '',
        },
        notExistingPackage
      )
    )
})

test('error is thrown when registry not responding', async () => {
  const notExistingPackage = 'foo'
  const notExistingRegistry = 'http://not-existing.pnpm.io'

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    retry: { retries: 1 },
    registries: {
      default: notExistingRegistry,
    },
  })
  await expect(resolveFromNpm({ alias: notExistingPackage, bareSpecifier: '1.0.0' }, {})).rejects
    .toThrow(new PnpmError('META_FETCH_FAIL', `GET ${notExistingRegistry}/${notExistingPackage}: request to ${notExistingRegistry}/${notExistingPackage} failed, reason: getaddrinfo ENOTFOUND not-existing.pnpm.io`, { attempts: 1 }))
})

test('extra info is shown if package has valid semver appended', async () => {
  const notExistingPackage = 'foo1.0.0'

  nock(registries.default)
    .get(`/${notExistingPackage}`)
    .reply(404, {})

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  await expect(resolveFromNpm({ alias: notExistingPackage, bareSpecifier: '1.0.0' }, {})).rejects
    .toThrow(
      new RegistryResponseError(
        {
          url: `${registries.default}${notExistingPackage}`,
        },
        {
          status: 404,
          // statusText: 'Not Found',
          statusText: '',
        },
        notExistingPackage
      )
    )
})

test('error is thrown when there is no package found for the requested version', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const wantedDependency = { alias: 'is-positive', bareSpecifier: '1000.0.0' }
  await expect(resolveFromNpm(wantedDependency, {})).rejects
    .toThrow(
      new NoMatchingVersionError({
        wantedDependency,
        packageMeta: isPositiveMeta,
        registry: registries.default,
      })
    )
})

test('error is thrown when package needs authorization', async () => {
  nock(registries.default)
    .get('/needs-auth')
    .reply(403)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  await expect(resolveFromNpm({ alias: 'needs-auth', bareSpecifier: '*' }, {})).rejects
    .toThrow(
      new RegistryResponseError(
        {
          url: `${registries.default}needs-auth`,
        },
        {
          status: 403,
          // statusText: 'Forbidden',
          statusText: '',
        },
        'needs-auth'
      )
    )
})

test('error is thrown when there is no package found for the requested range', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const wantedDependency = { alias: 'is-positive', bareSpecifier: '^1000.0.0' }
  await expect(resolveFromNpm(wantedDependency, {})).rejects
    .toThrow(
      new NoMatchingVersionError({
        wantedDependency,
        packageMeta: isPositiveMeta,
        registry: registries.default,
      })
    )
})

test('error is thrown when there is no package found for the requested tag', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const wantedDependency = { alias: 'is-positive', bareSpecifier: 'unknown-tag' }
  await expect(resolveFromNpm(wantedDependency, {})).rejects
    .toThrow(
      new NoMatchingVersionError({
        wantedDependency,
        packageMeta: isPositiveMeta,
        registry: registries.default,
      })
    )
})

test('resolveFromNpm() loads full metadata even if non-full metadata is already cached in store', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)
    .get('/is-positive')
    .reply(200, isPositiveMetaFull)

  const cacheDir = temporaryDirectory()

  {
    const { resolveFromNpm } = createResolveFromNpm({
      fullMetadata: false,
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })
    const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
    expect(resolveResult!.manifest!['scripts']).toBeFalsy()
  }

  {
    const { resolveFromNpm } = createResolveFromNpm({
      fullMetadata: true,
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })
    const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
    expect(resolveResult!.manifest!['scripts']).toBeTruthy()
  }
})

test('resolve when tarball URL is requested from the registry', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: `${registries.default}is-positive/-/is-positive-1.0.0.tgz`,
  }, {
    calcSpecifier: true,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
  expect(resolveResult!.normalizedBareSpecifier).toBe(`${registries.default}is-positive/-/is-positive-1.0.0.tgz`)

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadMsgpackFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'registry.npmjs.org/is-positive.mpk')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolve when tarball URL is requested from the registry and alias is not specified', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ bareSpecifier: `${registries.default}is-positive/-/is-positive-1.0.0.tgz` }, { calcSpecifier: true })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
  expect(resolveResult!.normalizedBareSpecifier).toBe(`${registries.default}is-positive/-/is-positive-1.0.0.tgz`)

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadMsgpackFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'registry.npmjs.org/is-positive.mpk')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolve from local directory when it matches the latest version of the package', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve injected dependency from local directory when it matches the latest version of the package', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', injected: true, bareSpecifier: '1.0.0' }, {
    projectDir: '/home/istvan/src',
    lockfileDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
      ])],
    ]),
  })

  // Injected workspace dependencies should still signal that they're resolved
  // via the 'workspace' rather than 'local-filesystem'.
  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('file:is-positive')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: 'is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('do not resolve from local directory when alwaysTryWorkspacePackages is false', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when alwaysTryWorkspacePackages is false but workspace: is used', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'workspace:*' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when alwaysTryWorkspacePackages is false but workspace: is used with a different package name', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', bareSpecifier: 'workspace:is-positive@*' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('use version from the registry if it is newer than the local one', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.1.0')
})

test('preferWorkspacePackages: use version from the workspace even if there is newer version in the registry', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    preferWorkspacePackages: true,
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult).toStrictEqual(
    expect.objectContaining({
      resolvedVia: 'workspace',
      id: 'link:is-positive',
      latest: '3.1.0',
    })
  )
})

test('use local version if it is newer than the latest in the registry', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    bareSpecifier: '^3.0.0',
  }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.2.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.2.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.2.0')
})

test('resolve from local directory when package is not found in the registry', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1' }, {
    projectDir: '/home/istvan/src/foo',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive-1.0.0' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
        ['1.1.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.1.0',
          },
        }],
        ['2.0.0', {
          rootDir: '/home/istvan/src/is-positive-2.0.0' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '2.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:../is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.1.0')
})

test('resolve from local directory when package is not found in the registry and latest installed', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'latest' }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive-1.0.0' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
        ['1.1.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.1.0',
          },
        }],
        ['2.0.0', {
          rootDir: '/home/istvan/src/is-positive-2.0.0' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '2.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive-2.0.0')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive-2.0.0',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('2.0.0')
})

test('resolve from local directory when package is not found in the registry and local prerelease available', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'latest' }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.0.0-alpha.1.2.3', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.0.0-alpha.1.2.3',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.0.0-alpha.1.2.3')
})

test('resolve from local directory when package is not found in the registry and specific version is requested', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.1.0' }, {
    projectDir: '/home/istvan/src/foo',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive-1.0.0' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
        ['1.1.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.1.0',
          },
        }],
        ['2.0.0', {
          rootDir: '/home/istvan/src/is-positive-2.0.0' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '2.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:../is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.1.0')
})

test('resolve from local directory when the requested version is not found in the registry but is available locally', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '100.0.0' }, {
    projectDir: '/home/istvan/src/foo',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['100.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '100.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:../is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('100.0.0')
})

test('workspace protocol: resolve from local directory even when it does not match the latest version of the package', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'workspace:^3.0.0' }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.0.0')
})

test('workspace protocol: resolve from local package that has a pre-release version', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'workspace:*' }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.0.0-alpha.1.2.3', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.0.0-alpha.1.2.3',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('workspace')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.0.0-alpha.1.2.3')
})

test("workspace protocol: don't resolve from local package that has a pre-release version that don't satisfy the range", async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '2' }, {
    projectDir: '/home/istvan/src',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['3.0.0-alpha.1.2.3', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '3.0.0-alpha.1.2.3',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@2.0.0')
  expect(resolveResult!.latest).toBeTruthy()
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('2.0.0')
})

test('workspace protocol: resolution fails if there is no matching local package', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  const projectDir = '/home/istvan/src'
  let err!: Error & { code: string }
  try {
    await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'workspace:^3.0.0' }, {
      projectDir,
      workspacePackages: new Map(),
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_WORKSPACE_PKG_NOT_FOUND')
  expect(err.message).toBe(`In ${path.relative(process.cwd(), projectDir)}: "is-positive@workspace:^3.0.0" is in the dependencies but no package named "is-positive" is present in the workspace`)
})

test('workspace protocol: resolution fails if there is no matching local package version', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  const projectDir = '/home/istvan/src'
  let err!: Error & { code: string }
  try {
    await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'workspace:^3.0.0' }, {
      projectDir,
      workspacePackages: new Map([
        ['is-positive', new Map([
          ['2.0.0', {
            rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
            manifest: {
              name: 'is-positive',
              version: '2.0.0',
            },
          }],
        ])],
      ]),
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE')
  expect(err.message).toBe(`In ${path.relative(process.cwd(), projectDir)}: No matching version found for is-positive@workspace:^3.0.0 inside the workspace. Available versions: 2.0.0`)
})

test('workspace protocol: resolution fails if there are no local packages', async () => {
  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  let err!: Error
  try {
    await resolveFromNpm({ alias: 'is-positive', bareSpecifier: 'workspace:^3.0.0' }, {
      projectDir: '/home/istvan/src',
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.message).toBe('Cannot resolve package from workspace because opts.workspacePackages is not defined')
})

test('throws error when package name has "/" but not starts with @scope', async () => {
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registries,
  })
  await expect(resolveFromNpm({ alias: 'regenerator/runtime' }, {})).rejects
    .toThrow(
      new PnpmError('INVALID_PACKAGE_NAME', 'Package name regenerator/runtime is invalid, it should have a @scope')
    )
})

test('resolveFromNpm() should always return the name of the package that is specified in the root of the meta', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveBrokenMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '3.1.0' }, {})

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.1.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadMsgpackFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'registry.npmjs.org/is-positive.mpk')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('request to metadata is retried if the received JSON is broken', async () => {
  const registries: Registries = {
    default: 'https://registry1.com/',
  }
  nock(registries.default)
    .get('/is-positive')
    .reply(200, '{')

  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    retry: { retries: 1 },
    storeDir: temporaryDirectory(),
    registries,
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})!

  expect(resolveResult?.id).toBe('is-positive@1.0.0')
})

test('request to a package with unpublished versions', async () => {
  nock(registries.default)
    .get('/code-snippet')
    .reply(200, loadJsonFileSync(f.find('unpublished.json')))

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  await expect(resolveFromNpm({ alias: 'code-snippet' }, {})).rejects
    .toThrow(
      new PnpmError('NO_VERSIONS', 'No versions available for code-snippet because it was unpublished')
    )
})

test('request to a package with no versions', async () => {
  nock(registries.default)
    .get('/code-snippet')
    .reply(200, { name: 'code-snippet' })

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  await expect(resolveFromNpm({ alias: 'code-snippet' }, {})).rejects
    .toThrow(
      new PnpmError('NO_VERSIONS', 'No versions available for code-snippet. The package may be unpublished.')
    )
})

test('request to a package with no dist-tags', async () => {
  const isPositiveMeta = omit(['dist-tags'], loadJsonFileSync<any>(f.find('is-positive.json'))) // eslint-disable-line
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  await expect(resolveFromNpm({ alias: 'is-positive' }, {})).rejects
    .toThrow(
      new PnpmError('MALFORMED_METADATA', 'Received malformed metadata for "is-positive"')
    )
})

test('resolveFromNpm() does not fail if the meta file contains no integrity information', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, brokenIntegrity)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '2.0.0' }, {})

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@2.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: undefined,
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-2.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('2.0.0')
})

test('resolveFromNpm() fails if the meta file contains invalid shasum', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, brokenIntegrity)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  await expect(
    resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})
  ).rejects.toThrow('Tarball "https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz" has invalid shasum specified in its metadata: a')
})

test('resolveFromNpm() should normalize the registry', async () => {
  nock('https://reg.com/owner')
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: path.join(cacheDir, 'store'),
    cacheDir,
    registries: {
      default: 'https://reg.com/owner',
    },
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {})

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.')).toHaveLength(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('pick lowest version by * when there are only prerelease versions', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, {
      versions: {
        '1.0.0-alpha.1': {
          name: 'is-positive',
          version: '1.0.0-alpha.1',
          dist: {
            tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0-alpha.1.tgz',
          },
        },
      },
      'dist-tags': {
        latest: '1.0.0-alpha.1',
      },
    })

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: path.join(cacheDir, 'store'),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '*' }, {
    pickLowestVersion: true,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0-alpha.1')
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0-alpha.1')
})

test('throws when workspace package version does not match and package is not found in the registry', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  await expect(
    resolveFromNpm({ alias: 'is-positive', bareSpecifier: '2.0.0' }, {
      projectDir: '/home/istvan/src',
      update: 'compatible',
      workspacePackages: new Map([
        ['is-positive', new Map([
          ['1.0.0', {
            rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
            manifest: {
              name: 'is-positive',
              version: '1.0.0',
            },
          }],
        ])],
      ]),
    })
  ).rejects.toThrow()
})

test('throws NoMatchingVersionError when workspace package version does not match and registry has no matching version', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })

  await expect(
    resolveFromNpm({ alias: 'is-positive', bareSpecifier: '99.0.0' }, {
      projectDir: '/home/istvan/src',
      update: 'compatible',
      workspacePackages: new Map([
        ['is-positive', new Map([
          ['1.0.0', {
            rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
            manifest: {
              name: 'is-positive',
              version: '1.0.0',
            },
          }],
        ])],
      ]),
    })
  ).rejects.toThrow(NoMatchingVersionError)
})

test('resolve from registry when workspace package version does not match the requested version', async () => {
  nock(registries.default)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '3.1.0' }, {
    projectDir: '/home/istvan/src',
    update: 'compatible',
    workspacePackages: new Map([
      ['is-positive', new Map([
        ['1.0.0', {
          rootDir: '/home/istvan/src/is-positive' as ProjectRootDir,
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        }],
      ])],
    ]),
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('offline mode should only resolve to versions present in cachedVersions, ignoring nonCachedVersions', async () => {
  const cacheDir = temporaryDirectory()
  const storeDir = temporaryDirectory()
  const pkgName = 'xyz-offline-test'

  nock(registries.default)
    .get(`/${pkgName}`)
    .reply(200, {
      name: pkgName,
      'dist-tags': { latest: '2.1.1' },
      versions: {
        '2.0.0': {
          name: pkgName,
          version: '2.0.0',
          dist: { tarball: 'http://fake.url/2.0.0.tgz', shasum: 'fake' },
        },
        '2.1.1': {
          name: pkgName,
          version: '2.1.1',
          dist: { tarball: 'http://fake.url/2.1.1.tgz', shasum: 'fake' },
        },
      },
      time: {
        '2.0.0': '2024-01-01T00:00:00.000Z',
        '2.1.1': '2024-02-01T00:00:00.000Z',
      },
      cachedVersions: ['2.0.0'],
    })

  const onlineResolver = createResolveFromNpm({
    storeDir,
    cacheDir,
    registries,
  })

  await onlineResolver.resolveFromNpm(
    { alias: pkgName, bareSpecifier: '^2.0.0' },
    { calcSpecifier: true }
  )

  const offlineResolver = createResolveFromNpm({
    storeDir,
    cacheDir,
    registries,
    offline: true,
  })

  const resultA = await offlineResolver.resolveFromNpm(
    { alias: pkgName, bareSpecifier: '^2.0.0' },
    { calcSpecifier: true }
  )

  expect(resultA).toBeDefined()
  expect(resultA!.manifest!.version).toBe('2.0.0')

  await expect(
    offlineResolver.resolveFromNpm(
      { alias: pkgName, bareSpecifier: '^2.1.0' },
      { calcSpecifier: true }
    )
  ).rejects.toThrow(PnpmError)
})

test('offline mode should fail immediately with NO_OFFLINE_TARBALL if cachedVersions is empty', async () => {
  const cacheDir = temporaryDirectory()
  const storeDir = temporaryDirectory()
  const pkgName = 'xyz-empty-cache-test'

  nock(registries.default)
    .get(`/${pkgName}`)
    .reply(200, {
      name: pkgName,
      'dist-tags': { latest: '2.1.1' },
      versions: {
        '2.1.1': {
          name: pkgName,
          version: '2.1.1',
          dist: { tarball: 'http://fake.url/2.1.1.tgz', shasum: 'fake' },
        },
      },
      cachedVersions: [],
    })

  const onlineResolver = createResolveFromNpm({ storeDir, cacheDir, registries })
  await onlineResolver.resolveFromNpm({ alias: pkgName, bareSpecifier: '^2.0.0' }, { calcSpecifier: true })

  const offlineResolver = createResolveFromNpm({ storeDir, cacheDir, registries, offline: true })

  await expect(
    offlineResolver.resolveFromNpm(
      { alias: pkgName, bareSpecifier: '^2.0.0' },
      { calcSpecifier: true }
    )
  ).rejects.toThrow(PnpmError)
})
