import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import type { Registries } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('metadata is fetched again after calling clearCache()', async () => {
  const name = 'test-package'
  const meta: PackageMeta = {
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        // Generated locally through: echo '1.1.0-beta' | sha1sum
        dist: { shasum: '8c6981d7f982c3e2986fda2f34282264a4db344c', tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz` },
      },
    },
    'dist-tags': {
      latest: '3.0.0',
    },
    time: {
      '3.0.0': '2020-02-01T00:00:00.000Z',
    },
  }

  const mockPool = getMockAgent().get('https://registry.npmjs.org')
  mockPool.intercept({ path: `/${name}`, method: 'GET' })
    .reply(200, meta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm, clearCache } = createResolveFromNpm({
    cacheDir,
    fullMetadata: true,
    registries,
    storeDir: temporaryDirectory(),
  })

  const res = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {})
  expect(res?.id).toBe(`${name}@3.0.0`)

  // Simulate publishing a new 3.1.0 version.
  meta.versions['3.1.0'] = {
    name,
    version: '3.1.0',
    dist: { shasum: '5f022945150b402cb3e470acc3818847b3dc5e00', tarball: `https://registry.npmjs.org/${name}/-/${name}-3.1.0.tgz` },
  }
  meta['dist-tags'].latest = '3.1.0'

  mockPool.intercept({ path: `/${name}`, method: 'GET' })
    .reply(200, meta)

  // Until the cache is cleared, the resolver will still return 3.0.0.
  const res2 = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {})
  expect(res2?.id).toBe(`${name}@3.0.0`)

  clearCache()

  // After clearing cache, the resolver should start returning 3.1.0.
  const res3 = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {})
  expect(res3?.id).toBe(`${name}@3.1.0`)
})
