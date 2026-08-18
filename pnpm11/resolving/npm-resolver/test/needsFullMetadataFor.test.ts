import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import type { RegistriesByScope } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

const PUBLIC_REGISTRY = 'https://registry.npmjs.org/'
const TIME_REGISTRY = 'https://time.example.com/'
const ABBREVIATED_ACCEPT = 'application/vnd.npm.install-v1+json'
const TIME_PKG = '@time/pkg'

const registriesByScope: RegistriesByScope = {
  default: PUBLIC_REGISTRY,
  '@time': TIME_REGISTRY,
}

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

function metaOf (name: string): PackageMeta {
  return {
    name,
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        dist: { shasum: '', tarball: `${PUBLIC_REGISTRY}${name}/-/pkg-1.0.0.tgz` },
      },
    },
    time: { '1.0.0': '2020-02-01T00:00:00.000Z' },
  }
}

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

/**
 * The public registry omits `time` from its abbreviated metadata, so a
 * time-based resolution has to read the full document from it. A registry that
 * declares `supportsTimeField` does not, and paying for the full document at
 * every registry because one of them needs it is the cost this removes.
 */
test('a registry that carries the time field is asked for abbreviated metadata, the others for full', async () => {
  const accepts: Record<string, string | undefined> = {}
  const capture = (registry: string) => (opts: { headers?: unknown }): { statusCode: number, data: PackageMeta } => {
    const headers = opts.headers as Record<string, string>
    accepts[registry] = headers.accept ?? headers.Accept
    return {
      statusCode: 200,
      data: metaOf(registry === 'public' ? 'pkg' : TIME_PKG),
    }
  }

  getMockAgent().get('https://registry.npmjs.org')
    .intercept({ path: '/pkg', method: 'GET' })
    .reply(capture('public'))
  getMockAgent().get('https://time.example.com')
    .intercept({ path: `/@${encodeURIComponent(TIME_PKG.slice(1))}`, method: 'GET' })
    .reply(capture('time'))

  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registriesByScope,
    // What `needsFullMetadataForRegistry` answers for a time-based resolution
    // where only the second registry declares `supportsTimeField: true`.
    needsFullMetadataFor: (registry: string) => registry !== TIME_REGISTRY,
  })

  await resolveFromNpm({ alias: 'pkg', bareSpecifier: '1.0.0' }, {})
  await resolveFromNpm({ alias: TIME_PKG, bareSpecifier: '1.0.0' }, {})

  expect(accepts.public).not.toContain(ABBREVIATED_ACCEPT)
  expect(accepts.time).toContain(ABBREVIATED_ACCEPT)
})
