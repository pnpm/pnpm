/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import {
  createNpmResolver,
  RegistryResponseError,
  NoMatchingVersionError,
} from '@pnpm/npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectRootDir } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import omit from 'ramda/src/omit'
import tempy from 'tempy'

const f = fixtures(__dirname)
/* eslint-disable @typescript-eslint/no-explicit-any */
const isPositiveMeta = loadJsonFile.sync<any>(f.find('is-positive.json'))
const isPositiveMetaWithDeprecated = loadJsonFile.sync<any>(f.find('is-positive-with-deprecated.json'))
const isPositiveMetaFull = loadJsonFile.sync<any>(f.find('is-positive-full.json'))
const isPositiveBrokenMeta = loadJsonFile.sync<any>(f.find('is-positive-broken.json'))
const sindresorhusIsMeta = loadJsonFile.sync<any>(f.find('sindresorhus-is.json'))
const jsonMeta = loadJsonFile.sync<any>(f.find('JSON.json'))
const brokenIntegrity = loadJsonFile.sync<any>(f.find('broken-integrity.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const registry = 'https://registry.npmjs.org/'

const delay = async (time: number) => new Promise<void>((resolve) => setTimeout(() => {
  resolve()
}, time))

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

async function retryLoadJsonFile<T> (filePath: string) {
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    try {
      return await loadJsonFile<T>(filePath)
    } catch (err: any) { // eslint-disable-line
      if (retry > 2) throw err
      retry++
    }
  }
  /* eslint-enable no-await-in-loop */
}

afterEach(() => {
  nock.cleanAll()
  nock.disableNetConnect()
})

beforeEach(() => {
  nock.enableNetConnect()
})

test('resolveFromNpm()', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, 'metadata/registry.npmjs.org/is-positive.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolveFromNpm() should save metadata to a unique file when the package name has upper case letters', async () => {
  nock(registry)
    .get('/JSON')
    .reply(200, jsonMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'JSON', pref: '1.0.0' }, {
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('JSON@1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, 'metadata/registry.npmjs.org/JSON_0ecd11c1d7a287401d148a23bbd7a2f8.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('relative workspace protocol is skipped', async () => {
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ pref: 'workspace:../is-positive' }, {
    projectDir: '/home/istvan/src',
    registry,
  })

  expect(resolveResult).toBe(null)
})

test('dry run', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
    dryRun: true,
    registry,
  })

  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
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

test('resolve to latest when no pref specified', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('resolve to defaultTag when no pref specified', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive' }, {
    defaultTag: 'stable',
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('resolve to biggest non-deprecated version that satisfies the range', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMetaWithDeprecated)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '3' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('resolve to a deprecated version if there are no non-deprecated ones that satisfy the range', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMetaWithDeprecated)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '2' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@2.0.0')
})

test('can resolve aliased dependency', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', pref: 'npm:is-positive@1.0.0' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('can resolve aliased dependency w/o version specifier', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', pref: 'npm:is-positive' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('can resolve aliased dependency w/o version specifier to default tag', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', pref: 'npm:is-positive' }, {
    defaultTag: 'stable',
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('can resolve aliased scoped dependency', async () => {
  nock(registry)
    .get('/@sindresorhus%2Fis')
    .reply(200, sindresorhusIsMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is', pref: 'npm:@sindresorhus/is@0.6.0' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('@sindresorhus/is@0.6.0')
})

test('can resolve aliased scoped dependency w/o version specifier', async () => {
  nock(registry)
    .get('/@sindresorhus%2Fis')
    .reply(200, sindresorhusIsMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is', pref: 'npm:@sindresorhus/is' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('@sindresorhus/is@0.7.0')
})

test('can resolve package with version prefixed with v', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: 'v1.0.0' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('can resolve package version loosely', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '= 1.0.0' }, {
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test("resolves to latest if it's inside the wanted range. Even if there are newer versions available inside the range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
  })

  // 3.1.0 is available but latest is 3.0.0, so preferring it
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("resolves to latest if it's inside the preferred range. Even if there are newer versions available inside the preferred range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^3.0.0': 'range' },
    },
    registry,
  })

  // 3.1.0 is available but latest is 3.0.0, so preferring it
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("resolve using the wanted range, when it doesn't intersect with the preferred range. Even if the preferred range contains the latest version", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '2.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^2.0.0': 'range' },
    },
    registry,
  })

  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("use the preferred version if it's inside the wanted range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '3.0.0': 'version' },
    },
    registry,
  })

  // 3.1.0 is the latest but we prefer the 3.0.0
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("ignore the preferred version if it's not inside the wanted range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '2.0.0': 'version' },
    },
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('use the preferred range if it intersects with the wanted range', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '1.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '>=1.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^3.0.0': 'range' },
    },
    registry,
  })

  // 1.0.0 is the latest but we prefer a version that is also in the preferred range
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('use the preferred range if it intersects with the wanted range (an array of preferred versions is passed)', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '1.0.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '>=1.0.0',
  }, {
    preferredVersions: {
      'is-positive': {
        '3.0.0': 'version',
        '3.1.0': 'version',
      },
    },
    registry,
  })

  // 1.0.0 is the latest but we prefer a version that is also in the preferred range
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("ignore the preferred range if it doesn't intersect with the wanted range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^2.0.0': 'range' },
    },
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("use the preferred dist-tag if it's inside the wanted range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.1.0',
        stable: '3.0.0',
      },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { stable: 'tag' },
    },
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test("ignore the preferred dist-tag if it's not inside the wanted range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.1.0',
        stable: '2.0.0',
      },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { stable: 'tag' },
    },
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test("prefer a version that is both inside the wanted and preferred ranges. Even if it's not the latest of any of them", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.0.0',
      },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '1.0.0 || 2.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '1.0.0 || 3.0.0': 'range' },
    },
    registry,
  })
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('prefer the version that is matched by more preferred selectors', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': { '^3.0.0': 'range', '3.0.0': 'version' },
    },
    registry,
  })

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('prefer the version that has bigger weight in preferred selectors', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferredVersions: {
      'is-positive': {
        '^3.0.0': 'range',
        '3.0.0': { selectorType: 'version', weight: 100 },
        '3.1.0': 'version',
      },
    },
    registry,
  })

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('offline resolution fails when package meta not found in the store', async () => {
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    offline: true,
    cacheDir,
  })

  await expect(resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, { registry })).rejects
    .toThrow(
      new PnpmError('NO_OFFLINE_META', `Failed to resolve is-positive@1.0.0 in package mirror ${path.join(cacheDir, 'metadata/registry.npmjs.org/is-positive.json')}`)
    )
})

test('offline resolution succeeds when package meta is found in the store', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()

  {
    const { resolveFromNpm } = createResolveFromNpm({
      offline: false,
      cacheDir,
    })

    // This request will save the package's meta in the store
    await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  }

  {
    const { resolveFromNpm } = createResolveFromNpm({
      offline: true,
      cacheDir,
    })

    const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, { registry })
    expect(resolveResult!.id).toBe('is-positive@1.0.0')
  }
})

test('prefer offline resolution does not fail when package meta not found in the store', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    preferOffline: true,
    cacheDir: tempy.directory(),
  })

  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
})

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const cacheDir = tempy.directory()

  {
    const { resolveFromNpm } = createResolveFromNpm({
      cacheDir,
    })

    // This request will save the package's meta in the store
    await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  }

  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  {
    const { resolveFromNpm } = createResolveFromNpm({
      preferOffline: true,
      cacheDir,
    })

    const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '^3.0.0' }, { registry })
    expect(resolveResult!.id).toBe('is-positive@3.0.0')
  }

  nock.cleanAll()
})

test('error is thrown when package is not found in the registry', async () => {
  const notExistingPackage = 'foo'

  nock(registry)
    .get(`/${notExistingPackage}`)
    .reply(404, {})

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  await expect(resolveFromNpm({ alias: notExistingPackage, pref: '1.0.0' }, { registry })).rejects
    .toThrow(
      new RegistryResponseError(
        {
          url: `${registry}${notExistingPackage}`,
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
    cacheDir: tempy.directory(),
    retry: { retries: 1 },
  })
  await expect(resolveFromNpm({ alias: notExistingPackage, pref: '1.0.0' }, { registry: notExistingRegistry })).rejects
    .toThrow(new PnpmError('META_FETCH_FAIL', `GET ${notExistingRegistry}/${notExistingPackage}: request to ${notExistingRegistry}/${notExistingPackage} failed, reason: getaddrinfo ENOTFOUND not-existing.pnpm.io`, { attempts: 1 }))
})

test('extra info is shown if package has valid semver appended', async () => {
  const notExistingPackage = 'foo1.0.0'

  nock(registry)
    .get(`/${notExistingPackage}`)
    .reply(404, {})

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  await expect(resolveFromNpm({ alias: notExistingPackage, pref: '1.0.0' }, { registry })).rejects
    .toThrow(
      new RegistryResponseError(
        {
          url: `${registry}${notExistingPackage}`,
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
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const wantedDependency = { alias: 'is-positive', pref: '1000.0.0' }
  await expect(resolveFromNpm(wantedDependency, { registry })).rejects
    .toThrow(
      new NoMatchingVersionError({
        wantedDependency,
        packageMeta: isPositiveMeta,
      })
    )
})

test('error is thrown when package needs authorization', async () => {
  nock(registry)
    .get('/needs-auth')
    .reply(403)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  await expect(resolveFromNpm({ alias: 'needs-auth', pref: '*' }, { registry })).rejects
    .toThrow(
      new RegistryResponseError(
        {
          url: `${registry}needs-auth`,
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
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const wantedDependency = { alias: 'is-positive', pref: '^1000.0.0' }
  await expect(resolveFromNpm(wantedDependency, { registry })).rejects
    .toThrow(
      new NoMatchingVersionError({
        wantedDependency,
        packageMeta: isPositiveMeta,
      })
    )
})

test('error is thrown when there is no package found for the requested tag', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const wantedDependency = { alias: 'is-positive', pref: 'unknown-tag' }
  await expect(resolveFromNpm(wantedDependency, { registry })).rejects
    .toThrow(
      new NoMatchingVersionError({
        wantedDependency,
        packageMeta: isPositiveMeta,
      })
    )
})

test('resolveFromNpm() loads full metadata even if non-full metadata is already cached in store', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)
    .get('/is-positive')
    .reply(200, isPositiveMetaFull)

  const cacheDir = tempy.directory()

  {
    const { resolveFromNpm } = createResolveFromNpm({
      fullMetadata: false,
      cacheDir,
    })
    const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
      registry,
    })
    expect(resolveResult!.manifest!['scripts']).toBeFalsy()
  }

  {
    const { resolveFromNpm } = createResolveFromNpm({
      fullMetadata: true,
      cacheDir,
    })
    const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
      registry,
    })
    expect(resolveResult!.manifest!['scripts']).toBeTruthy()
  }
})

test('resolve when tarball URL is requested from the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: `${registry}is-positive/-/is-positive-1.0.0.tgz` }, {
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
  expect(resolveResult!.normalizedPref).toBe(`${registry}is-positive/-/is-positive-1.0.0.tgz`)

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, 'metadata/registry.npmjs.org/is-positive.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolve when tarball URL is requested from the registry and alias is not specified', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ pref: `${registry}is-positive/-/is-positive-1.0.0.tgz` }, {
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
  expect(resolveResult!.normalizedPref).toBe(`${registry}is-positive/-/is-positive-1.0.0.tgz`)

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, 'metadata/registry.npmjs.org/is-positive.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('resolve from local directory when it matches the latest version of the package', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve injected dependency from local directory when it matches the latest version of the package', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', injected: true, pref: '1.0.0' }, {
    projectDir: '/home/istvan/src',
    lockfileDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('file:is-positive')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: 'is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('do not resolve from local directory when alwaysTryWorkspacePackages is false', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    registry,
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
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when alwaysTryWorkspacePackages is false but workspace: is used', async () => {
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: 'workspace:*' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'positive', pref: 'workspace:is-positive@*' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    projectDir: '/home/istvan/src',
    registry,
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
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.1.0')
})

test('preferWorkspacePackages: use version from the workspace even if there is newer version in the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferWorkspacePackages: true,
    projectDir: '/home/istvan/src',
    registry,
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
      resolvedVia: 'local-filesystem',
      id: 'link:is-positive',
      latest: '3.1.0',
    })
  )
})

test('use local version if it is newer than the latest in the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.2.0')
})

test('resolve from local directory when package is not found in the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1' }, {
    projectDir: '/home/istvan/src/foo',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: 'latest' }, {
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: 'latest' }, {
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.1.0' }, {
    projectDir: '/home/istvan/src/foo',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '100.0.0' }, {
    projectDir: '/home/istvan/src/foo',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: 'workspace:*' }, {
    projectDir: '/home/istvan/src',
    registry,
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

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
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
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '2' }, {
    projectDir: '/home/istvan/src',
    registry,
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
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })

  const projectDir = '/home/istvan/src'
  let err!: Error & { code: string }
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
      projectDir,
      registry,
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
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })

  const projectDir = '/home/istvan/src'
  let err!: Error & { code: string }
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
      projectDir,
      registry,
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
  expect(err.message).toBe(`In ${path.relative(process.cwd(), projectDir)}: No matching version found for is-positive@workspace:^3.0.0 inside the workspace`)
})

test('workspace protocol: resolution fails if there are no local packages', async () => {
  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })

  let err!: Error
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
      projectDir: '/home/istvan/src',
      registry,
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.message).toBe('Cannot resolve package from workspace because opts.workspacePackages is not defined')
})

test('throws error when package name has "/" but not starts with @scope', async () => {
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  await expect(resolveFromNpm({ alias: 'regenerator/runtime' }, { registry })).rejects
    .toThrow(
      new PnpmError('INVALID_PACKAGE_NAME', 'Package name regenerator/runtime is invalid, it should have a @scope')
    )
})

test('resolveFromNpm() should always return the name of the package that is specified in the root of the meta', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveBrokenMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '3.1.0' }, {
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.1.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, 'metadata/registry.npmjs.org/is-positive.json')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta.name).toBeTruthy()
  expect(meta.versions).toBeTruthy()
  expect(meta['dist-tags']).toBeTruthy()
})

test('request to metadata is retried if the received JSON is broken', async () => {
  const registry = 'https://registry1.com/'
  nock(registry)
    .get('/is-positive')
    .reply(200, '{')

  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    retry: { retries: 1 },
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
    registry,
  })!

  expect(resolveResult?.id).toBe('is-positive@1.0.0')
})

test('request to a package with unpublished versions', async () => {
  nock(registry)
    .get('/code-snippet')
    .reply(200, loadJsonFile.sync(f.find('unpublished.json')))

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({ cacheDir })

  await expect(resolveFromNpm({ alias: 'code-snippet' }, { registry })).rejects
    .toThrow(
      new PnpmError('NO_VERSIONS', 'No versions available for code-snippet because it was unpublished')
    )
})

test('request to a package with no versions', async () => {
  nock(registry)
    .get('/code-snippet')
    .reply(200, { name: 'code-snippet' })

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({ cacheDir })

  await expect(resolveFromNpm({ alias: 'code-snippet' }, { registry })).rejects
    .toThrow(
      new PnpmError('NO_VERSIONS', 'No versions available for code-snippet. The package may be unpublished.')
    )
})

test('request to a package with no dist-tags', async () => {
  const isPositiveMeta = omit(['dist-tags'], loadJsonFile.sync<any>(f.find('is-positive.json'))) // eslint-disable-line
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({ cacheDir })

  await expect(resolveFromNpm({ alias: 'is-positive' }, { registry })).rejects
    .toThrow(
      new PnpmError('MALFORMED_METADATA', 'Received malformed metadata for "is-positive"')
    )
})

test('resolveFromNpm() does not fail if the meta file contains no integrity information', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, brokenIntegrity)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '2.0.0' }, {
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@2.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: undefined,
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-2.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('2.0.0')
})

test('resolveFromNpm() fails if the meta file contains invalid shasum', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, brokenIntegrity)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  await expect(
    resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  ).rejects.toThrow('Tarball "https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz" has invalid shasum specified in its metadata: a')
})

test('resolveFromNpm() should normalize the registry', async () => {
  nock('https://reg.com/owner')
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '1.0.0' }, {
    registry: 'https://reg.com/owner',
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('pick lowest version by * when there are only prerelease versions', async () => {
  nock(registry)
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

  const cacheDir = tempy.directory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', pref: '*' }, {
    pickLowestVersion: true,
    registry,
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('is-positive@1.0.0-alpha.1')
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0-alpha.1')
})
