/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createResolver } from '@pnpm/resolving.default-resolver'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

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

test.each([
  ['link:./pkg', 'link'],
  ['workspace:./pkg', 'workspace'],
  ['file:./pkg', 'file'],
])('createResolver() lets the explicit local protocol %s win over a colliding named-registry alias', async (bareSpecifier, alias) => {
  const projectDir = temporaryDirectory()
  fs.mkdirSync(path.join(projectDir, 'pkg'))
  fs.writeFileSync(
    path.join(projectDir, 'pkg', 'package.json'),
    JSON.stringify({ name: 'pkg', version: '1.0.0' })
  )

  const { resolve } = createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registries,
    namedRegistries: {
      [alias]: ENTERPRISE_REGISTRY,
    },
  })

  const result = await resolve(
    { bareSpecifier },
    { lockfileDir: projectDir, projectDir, preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('local-filesystem')
})
