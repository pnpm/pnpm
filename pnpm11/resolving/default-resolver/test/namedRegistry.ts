/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
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

const registriesByScope = {
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
    registriesByScope,
  })

  const result = await resolve(
    { bareSpecifier: 'gh:@acme/private' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('named-registry')
  expect(result.id).toBe('@acme/private@gh:2.1.0')
})

test('createResolver() routes a user-configured named registry alias through the named-registry resolver', async () => {
  interceptAcmePrivate(ENTERPRISE_REGISTRY)

  const { resolve } = createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      work: ENTERPRISE_REGISTRY,
    }),
  })

  const result = await resolve(
    { bareSpecifier: 'work:@acme/private' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('named-registry')
  expect(result.id).toBe('@acme/private@work:2.1.0')
})

test.each([
  ['link'],
  ['workspace'],
  ['file'],
  ['runtime'],
])('createResolver() rejects the reserved named-registry alias %s', (alias) => {
  expect(() => createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      [alias]: ENTERPRISE_REGISTRY,
    }),
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME' }))
})

test('createResolver() rejects a malformed named-registry alias', () => {
  expect(() => createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      'no colons:allowed': ENTERPRISE_REGISTRY,
    }),
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME' }))
})

test('createResolver() qualifies a named-registry id with the registry alias', async () => {
  interceptAcmePrivate(ENTERPRISE_REGISTRY)

  const { resolve } = createResolver(fetch, () => undefined, {
    cacheDir: temporaryDirectory(),
    storeDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      work: ENTERPRISE_REGISTRY,
    }),
  })

  const result = await resolve(
    { bareSpecifier: 'work:@acme/private' },
    { lockfileDir: '/test', projectDir: '/test', preferredVersions: {} }
  )

  expect(result.resolvedVia).toBe('named-registry')
  expect(result.id).toBe('@acme/private@work:2.1.0')
})
