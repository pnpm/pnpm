/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createResolver } from '@pnpm/resolving.default-resolver'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

// Re-use the GitHub Packages metadata fixture shipped with the npm-resolver
// tests. It's the same shape the named-registry resolver consumes.
/* eslint-disable @typescript-eslint/no-explicit-any */
const ghAcmePrivateMeta = loadJsonFileSync<any>(
  path.join(import.meta.dirname, '../../npm-resolver/test/fixtures/gh-acme-private.json')
)
/* eslint-enable @typescript-eslint/no-explicit-any */

const GH_REGISTRY = 'https://npm.pkg.github.com/'
const ENTERPRISE_REGISTRY = 'https://npm.enterprise.example.com/'

const registries = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
}

const fetch = createFetchFromRegistry({})

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

function interceptAcmePrivate (registry: string): void {
  const slash = '%2F'
  const pool = getMockAgent().get(registry.replace(/\/$/, ''))
  pool.intercept({ path: `/@acme${slash}private`, method: 'GET' }).reply(200, ghAcmePrivateMeta)
}

// Regression: before the fix, the local resolver claimed any spec containing
// `/` (e.g. `gh:@acme/private`) as a directory and emitted a "non-existent
// directory" warning. The named-registry resolver must run first.
test('createResolver() routes <alias>:@scope/pkg through the named-registry resolver instead of the local resolver', async () => {
  interceptAcmePrivate(GH_REGISTRY)

  const { resolve } = createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registries,
  })

  const result = await resolve(
    { bareSpecifier: 'gh:@acme/private' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('named-registry')
  expect(result.id).toBe('@acme/private@2.1.0')
})

test('createResolver() routes a user-configured named registry alias through the named-registry resolver', async () => {
  interceptAcmePrivate(ENTERPRISE_REGISTRY)

  const { resolve } = createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registries,
    namedRegistries: {
      work: ENTERPRISE_REGISTRY,
    },
  })

  const result = await resolve(
    { bareSpecifier: 'work:@acme/private' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('named-registry')
  expect(result.id).toBe('@acme/private@2.1.0')
})
