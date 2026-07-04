import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolver } from '@pnpm/resolving.npm-resolver'
import type { Registries } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, retryLoadJsonFile, setupMockAgent, teardownMockAgent } from './utils/index.js'

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

const ABBREVIATED_CONTENT_TYPE = 'application/vnd.npm.install-v1+json'

// A "full" registry document: what a registry that ignores the abbreviated
// Accept header (e.g. Azure DevOps Artifacts) serves. It carries top-level and
// per-version fields the resolver never reads.
function fullFooMeta (): Record<string, unknown> {
  return {
    _id: 'foo',
    _rev: '1-abc',
    readme: 'x'.repeat(1000),
    name: 'foo',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        dependencies: { bar: '^1.0.0' },
        devDependencies: { typescript: '^5.0.0' },
        scripts: { build: 'tsc', postinstall: 'node ./install.js' },
        exports: { '.': './index.js' },
        description: 'a package',
        dist: {
          tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
  }
}

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

test('a full document served for an abbreviated request is normalized before caching (registry ignored the Accept header)', async () => {
  const cacheDir = temporaryDirectory()

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/foo', method: 'GET' })
    // application/json (not the abbreviated content type) signals that the
    // registry ignored our abbreviated Accept header and served the full doc.
    .reply(200, fullFooMeta(), { headers: { 'content-type': 'application/json' } })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const res = await resolveFromNpm({ alias: 'foo', bareSpecifier: '^1.0.0' }, {})
  expect(res!.id).toBe('foo@1.0.0')

  const cachePath = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org/foo.jsonl`)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const saved = await retryLoadJsonFile<any>(cachePath)
  const savedVersion = saved.versions['1.0.0']

  // Install-irrelevant fields dropped.
  expect(saved._id).toBeUndefined()
  expect(saved.readme).toBeUndefined()
  expect(savedVersion.scripts).toBeUndefined()
  expect(savedVersion.exports).toBeUndefined()
  expect(savedVersion.description).toBeUndefined()
  // Install-relevant fields kept, so resolution is unchanged.
  expect(savedVersion.dependencies).toEqual({ bar: '^1.0.0' })
  expect(savedVersion.dist).toBeDefined()
})

test('a document served with the abbreviated content type is cached verbatim (registry honored the Accept header)', async () => {
  const cacheDir = temporaryDirectory()

  // The abbreviated document a spec-compliant registry (e.g. npm) returns. It
  // also carries a custom top-level field to prove the body is stored verbatim
  // (no re-serialization / stripping) on the happy path.
  const abbreviatedDoc = {
    name: 'foo',
    'dist-tags': { latest: '1.0.0' },
    _cacheUntouchedMarker: 'kept-verbatim',
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        dependencies: { bar: '^1.0.0' },
        dist: {
          tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
  }

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/foo', method: 'GET' })
    // Media types are case-insensitive and may carry parameters; both must
    // still be recognized as the abbreviated content type.
    .reply(200, abbreviatedDoc, { headers: { 'content-type': `${ABBREVIATED_CONTENT_TYPE.toUpperCase()}; charset=utf-8` } })

  const { resolveFromNpm } = createResolveFromNpm({
    storeDir: temporaryDirectory(),
    cacheDir,
    registries,
  })
  const res = await resolveFromNpm({ alias: 'foo', bareSpecifier: '^1.0.0' }, {})
  expect(res!.id).toBe('foo@1.0.0')

  const cachePath = path.join(cacheDir, `${ABBREVIATED_META_DIR}/registry.npmjs.org/foo.jsonl`)
  // The registry body is stored untouched: no field stripping and no
  // re-serialization on the honored-header happy path.
  const saved = await retryLoadJsonFile<{ _cacheUntouchedMarker?: string }>(cachePath)
  expect(saved._cacheUntouchedMarker).toBe('kept-verbatim')
})
