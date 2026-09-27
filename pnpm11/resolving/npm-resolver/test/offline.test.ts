import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { RegistriesByScope } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { setupMockAgent, teardownMockAgent } from './utils/index.js'

/* eslint-disable @typescript-eslint/no-explicit-any */
const isPositiveAbbreviatedMeta = loadJsonFileSync<any>(path.join(import.meta.dirname, 'fixtures/is-positive.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const registriesByScope: RegistriesByScope = {
  default: 'https://registry.npmjs.org/',
}

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

function seedMetaMirror (cacheDir: string, meta: PackageMeta): void {
  const mirrorDir = path.join(cacheDir, ABBREVIATED_META_DIR, 'https%3A+registry.npmjs.org')
  fs.mkdirSync(mirrorDir, { recursive: true })
  fs.writeFileSync(
    path.join(mirrorDir, 'is-positive.jsonl'),
    `${JSON.stringify({})}\n${JSON.stringify(meta)}`,
    'utf8'
  )
}

/**
 * Adds a package version to the store index the way a previous install
 * would have, so the resolver's store-presence checks can find it.
 */
function seedStoreWithVersion (storeDir: string, name: string, version: string, integrity: string): void {
  const storeIndex = new StoreIndex(storeDir)
  try {
    storeIndex.set(storeIndexKey(integrity, `${name}@${version}`), {
      files: new Map(),
      algo: 'sha512',
      manifest: { name, version },
    })
  } finally {
    storeIndex.close()
  }
}

test('offline resolution picks the highest version whose tarball is in the store', async () => {
  const cacheDir = temporaryDirectory()
  seedMetaMirror(cacheDir, isPositiveAbbreviatedMeta)
  const storeDir = temporaryDirectory()
  seedStoreWithVersion(storeDir, 'is-positive', '3.0.0', isPositiveAbbreviatedMeta.versions['3.0.0'].dist.integrity)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir,
    cacheDir,
    offline: true,
    registriesByScope,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {})

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('offline resolution keeps preferring the store-held version on a repeat pick from the in-memory cache', async () => {
  const cacheDir = temporaryDirectory()
  seedMetaMirror(cacheDir, isPositiveAbbreviatedMeta)
  const storeDir = temporaryDirectory()
  seedStoreWithVersion(storeDir, 'is-positive', '3.0.0', isPositiveAbbreviatedMeta.versions['3.0.0'].dist.integrity)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir,
    cacheDir,
    offline: true,
    registriesByScope,
  })
  await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {})
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {})

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})

test('offline resolution keeps the newest pick when the store already holds it', async () => {
  const cacheDir = temporaryDirectory()
  seedMetaMirror(cacheDir, isPositiveAbbreviatedMeta)
  const storeDir = temporaryDirectory()
  seedStoreWithVersion(storeDir, 'is-positive', '3.1.0', isPositiveAbbreviatedMeta.versions['3.1.0'].dist.integrity)

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir,
    cacheDir,
    offline: true,
    registriesByScope,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {})

  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('offline resolution falls back to the newest version when the store holds none of them', async () => {
  const cacheDir = temporaryDirectory()
  seedMetaMirror(cacheDir, isPositiveAbbreviatedMeta)
  const storeDir = temporaryDirectory()

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir,
    cacheDir,
    offline: true,
    registriesByScope,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '^3.0.0' }, {})

  // The pick is unchanged so the install fails with ERR_PNPM_NO_OFFLINE_TARBALL
  // as before; the resolver has no version to offer instead.
  expect(resolveResult!.id).toBe('is-positive@3.1.0')
})

test('offline resolution of an exact version is unaffected by store contents', async () => {
  const cacheDir = temporaryDirectory()
  seedMetaMirror(cacheDir, isPositiveAbbreviatedMeta)
  const storeDir = temporaryDirectory()

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir,
    cacheDir,
    offline: true,
    registriesByScope,
  })
  const resolveResult = await resolveFromNpm({ alias: 'is-positive', bareSpecifier: '3.0.0' }, {})

  expect(resolveResult!.id).toBe('is-positive@3.0.0')
})
