import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { type LogBase, streamParser } from '@pnpm/logger'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { Registries } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

const fooMeta = {
  name: 'foo',
  'dist-tags': { latest: '2.1.4' },
  versions: {
    '2.1.3': {
      name: 'foo',
      version: '2.1.3',
      dist: {
        integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
        tarball: 'https://registry.npmjs.org/foo/-/foo-2.1.3.tgz',
      },
    },
    '2.1.4': {
      name: 'foo',
      version: '2.1.4',
      dist: {
        integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
        tarball: 'https://registry.npmjs.org/foo/-/foo-2.1.4.tgz',
      },
    },
  },
  time: {
    '2.1.3': '2026-01-01T00:00:00.000Z',
    '2.1.4': '2026-07-14T12:00:00.000Z',
  },
}

const collectedWarnings: string[] = []

function collectWarnings (msg: LogBase & { message?: string }): void {
  if (msg.level === 'warn' && typeof msg.message === 'string') {
    collectedWarnings.push(msg.message)
  }
}

beforeEach(async () => {
  collectedWarnings.length = 0
  streamParser.on('data', collectWarnings as (msg: LogBase) => void)
  await setupMockAgent()
})

afterEach(async () => {
  streamParser.removeListener('data', collectWarnings as (msg: LogBase) => void)
  await teardownMockAgent()
})

// https://github.com/pnpm/pnpm/issues/13071
test('does not warn about a held-back update when minimumReleaseAge is the reason the newer version was not picked', async () => {
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/foo', method: 'GET' })
    .reply(200, fooMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    fullMetadata: true,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'foo', bareSpecifier: '^2.1.3' }, {
    updateRequested: true,
    publishedBy: new Date('2026-07-01T00:00:00.000Z'),
    preferredVersions: {
      foo: { '^2.1.3': 'range' },
    },
  })

  // 2.1.4 is inside the minimumReleaseAge window, so 2.1.3 is picked.
  expect(resolveResult!.id).toBe('foo@2.1.3')
  // The pick is fully explained by the maturity cutoff, so no warning about
  // manifests holding the update back should be printed.
  expect(collectedWarnings.filter(warning => warning.includes('was updated to'))).toStrictEqual([])
})

test('still warns about a held-back update when a manifest pin is the reason the newer version was not picked', async () => {
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/foo', method: 'GET' })
    .reply(200, fooMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    fullMetadata: true,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'foo', bareSpecifier: '^2.1.3' }, {
    updateRequested: true,
    preferredVersions: {
      foo: { '2.1.3': { selectorType: 'version', weight: 1000 } },
    },
  })

  expect(resolveResult!.id).toBe('foo@2.1.3')
  const heldBackWarnings = collectedWarnings.filter(warning => warning.includes('was updated to'))
  expect(heldBackWarnings).toHaveLength(1)
  expect(heldBackWarnings[0]).toContain('was updated to 2.1.3, not 2.1.4')
})

test('keeps the unfiltered baseline for a package excluded from the age gate via publishedByExclude', async () => {
  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/foo', method: 'GET' })
    .reply(200, fooMeta)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    fullMetadata: true,
    registries,
  })
  const resolveResult = await resolveFromNpm({ alias: 'foo', bareSpecifier: '^2.1.3' }, {
    updateRequested: true,
    publishedBy: new Date('2026-07-01T00:00:00.000Z'),
    publishedByExclude: () => true,
    preferredVersions: {
      foo: { '2.1.3': { selectorType: 'version', weight: 1000 } },
    },
  })

  // The pin holds the pick at 2.1.3, and the exclusion keeps 2.1.4 in the
  // baseline despite the publishedBy cutoff, so the warning is printed.
  expect(resolveResult!.id).toBe('foo@2.1.3')
  const heldBackWarnings = collectedWarnings.filter(warning => warning.includes('was updated to'))
  expect(heldBackWarnings).toHaveLength(1)
  expect(heldBackWarnings[0]).toContain('was updated to 2.1.3, not 2.1.4')
})
