import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import { fixtures } from '@pnpm/test-fixtures'
import type { RegistriesByScope } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

const testFixtures = fixtures(import.meta.dirname)
/* eslint-disable @typescript-eslint/no-explicit-any -- the fixture is an arbitrary registry document */
const acmePrivateMeta = loadJsonFileSync<any>(testFixtures.find('gh-acme-private.json'))
const jsrRusGreetMeta = loadJsonFileSync<any>(testFixtures.find('jsr-rus-greet.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const WORK_REGISTRY = 'https://npm.work.example.com/'
const OTHER_REGISTRY = 'https://npm.other.example.com/'

const fetch = createFetchFromRegistry({})

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

function interceptAcmePrivate (registry: string): void {
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: `/@acme${'%2F'}private`, method: 'GET' })
    .reply(200, acmePrivateMeta)
}

function createResolver (registriesByScope: Partial<RegistriesByScope>, registriesByPrefix?: Record<string, string>): ReturnType<typeof createNpmResolver> {
  return createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@jsr': 'https://npm.jsr.io/',
      ...registriesByScope,
    },
    registriesByPrefix,
  })
}

test('a plain range its scope routes to a registry with a declared prefix gets the prefixed key', async () => {
  interceptAcmePrivate(WORK_REGISTRY)
  // The prefix is spelled with another case and without the trailing slash.
  const { resolveFromNpm } = createResolver({ '@acme': WORK_REGISTRY }, { work: 'https://NPM.work.example.com' })

  const resolveResult = await resolveFromNpm({ alias: '@acme/private', bareSpecifier: '^2.0.0' }, {})

  expect(resolveResult).toMatchObject({
    resolvedVia: 'npm-registry',
    id: '@acme/private@work:2.1.0',
  })
})

test('a plain range resolved from the default registry gets the prefix the default registry declares', async () => {
  interceptAcmePrivate(WORK_REGISTRY)
  const { resolveFromNpm } = createResolver({ default: WORK_REGISTRY }, { work: WORK_REGISTRY })

  const resolveResult = await resolveFromNpm({ alias: '@acme/private', bareSpecifier: '^2.0.0' }, {})

  expect(resolveResult).toMatchObject({ id: '@acme/private@work:2.1.0' })
})

test('the same name and version from another registry keeps a plain key', async () => {
  interceptAcmePrivate(OTHER_REGISTRY)
  const { resolveFromNpm } = createResolver({ '@acme': OTHER_REGISTRY }, { work: WORK_REGISTRY })

  const resolveResult = await resolveFromNpm({ alias: '@acme/private', bareSpecifier: '^2.0.0' }, {})

  expect(resolveResult).toMatchObject({ id: '@acme/private@2.1.0' })
})

test('routing a scope to registry.npmjs.org does not turn its keys into npmjs: keys', async () => {
  interceptAcmePrivate('https://registry.npmjs.org/')
  const { resolveFromNpm } = createResolver({}, {})

  const resolveResult = await resolveFromNpm({ alias: '@acme/private', bareSpecifier: '^2.0.0' }, {})

  expect(resolveResult).toMatchObject({ id: '@acme/private@2.1.0' })
})

test('a named-registry specifier is keyed by the smallest prefix its registry declares', async () => {
  interceptAcmePrivate(WORK_REGISTRY)
  const { resolveFromNamedRegistry } = createResolver({}, { zeta: WORK_REGISTRY, work: WORK_REGISTRY })

  const resolveResult = await resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'zeta:^2.0.0' }, { calcSpecifier: true })

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'zeta',
    id: '@acme/private@work:2.1.0',
    // The specifier keeps the prefix it was written with.
    normalizedBareSpecifier: 'zeta:^2.1.0',
  })
})

test('a built-in alias keeps its own name in the key', async () => {
  interceptAcmePrivate('https://registry.npmjs.org/')
  const { resolveFromNamedRegistry } = createResolver({}, { work: WORK_REGISTRY })

  const resolveResult = await resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'npmjs:^2.0.0' }, {})

  expect(resolveResult).toMatchObject({ id: '@acme/private@npmjs:2.1.0' })
})

test('a jsr: specifier is keyed by the prefix the @jsr registry declares', async () => {
  getMockAgent().get(WORK_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: `/@jsr${'%2F'}rus__greet`, method: 'GET' })
    .reply(200, jsrRusGreetMeta)
  const { resolveFromJsr } = createResolver({ '@jsr': WORK_REGISTRY }, { work: WORK_REGISTRY })

  const resolveResult = await resolveFromJsr({ alias: '@rus/greet', bareSpecifier: 'jsr:0.0.3' }, {})

  expect(resolveResult).toMatchObject({
    resolvedVia: 'jsr-registry',
    id: '@jsr/rus__greet@work:0.0.3',
  })
})

test('a built-in alias pointing at a registry with a declared prefix is keyed by that prefix', async () => {
  interceptAcmePrivate('https://registry.npmjs.org/')
  const { resolveFromNamedRegistry } = createResolver({}, { public: 'https://registry.npmjs.org' })

  const resolveResult = await resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'npmjs:^2.0.0' }, {})

  expect(resolveResult).toMatchObject({ id: '@acme/private@public:2.1.0' })
})
