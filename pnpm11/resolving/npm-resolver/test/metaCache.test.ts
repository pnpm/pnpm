import { expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { temporaryDirectory } from 'tempy'

import type { RegistryPackageSpec } from '../src/parseBareSpecifier.js'
import {
  getPkgMetaCacheKey,
  getPkgMirrorPath,
  type PackageMetaCache,
  pickPackage,
  prepareJsonForDisk,
  saveMeta,
} from '../src/pickPackage.js'

const REGISTRY = 'https://registry.npmjs.org/'

function fooMeta (): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        dist: {
          tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
  } as unknown as PackageMeta
}

function createMetaCache (): PackageMetaCache {
  const store = new Map<string, PackageMeta>()
  return {
    get: (key) => store.get(key),
    set: (key, meta) => {
      store.set(key, meta)
    },
    has: (key) => store.has(key),
  }
}

test('getPkgMetaCacheKey canonicalizes the registry so trailing-slash variants share one key', () => {
  // A configured named registry without a trailing slash and the verifier's
  // trailing-slashed prefix for the same registry must map to one cache slot.
  expect(getPkgMetaCacheKey('https://reg.example.com', 'foo', false, false))
    .toBe(getPkgMetaCacheKey('https://reg.example.com/', 'foo', false, false))

  // Registries that genuinely differ by path are never collapsed.
  expect(getPkgMetaCacheKey('https://reg.example.com/team-a/', 'foo', false, false))
    .not.toBe(getPkgMetaCacheKey('https://reg.example.com/team-b/', 'foo', false, false))

  // Abbreviated and full documents keep distinct slots.
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', true, false))
    .not.toBe(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))

  // Filtered and unfiltered full metadata keep distinct slots (clearMeta
  // strips the filtered form), but filterMetadata is irrelevant to the
  // abbreviated key.
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', true, true))
    .not.toBe(getPkgMetaCacheKey(REGISTRY, 'foo', true, false))
  expect(getPkgMetaCacheKey(REGISTRY, 'foo', false, true))
    .toBe(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))
})

test('updateChecksums bypasses the in-memory cache so a disk-promoted entry cannot skip revalidation', async () => {
  const meta = fooMeta()
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, undefined))

  const fetchedNames: string[] = []
  const ctx = {
    fetch: async (pkgName: string) => {
      fetchedNames.push(pkgName)
      return { meta, jsonText: JSON.stringify(meta), etag: undefined }
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'version', name: 'foo', fetchSpec: '1.0.0' }

  // A normal resolve takes the on-disk exact-version fast path: no network, and
  // it promotes the disk-loaded packument into the in-memory cache.
  const first = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  expect(first.pickedPackage?.version).toBe('1.0.0')
  expect(fetchedNames).toHaveLength(0)
  expect(ctx.metaCache.has(getPkgMetaCacheKey(REGISTRY, 'foo', false, false))).toBe(true)

  // updateChecksums must still hit the registry, even though the warm in-memory
  // cache now holds a disk-sourced entry for this package.
  await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, updateChecksums: true, preferredVersionSelectors: undefined })
  expect(fetchedNames).toEqual(['foo'])
})

test('abbreviated metadata is normalized to the abbreviated field set before caching', async () => {
  // Simulate a registry (e.g. Azure DevOps Artifacts) that ignores the
  // abbreviated Accept header and returns the full package.json per version,
  // including fields the resolver never reads.
  const meta = {
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
  } as unknown as PackageMeta

  const cacheDir = temporaryDirectory()
  const ctx = {
    fetch: async () => ({ meta, jsonText: JSON.stringify(meta), etag: undefined }),
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '^1.0.0' }

  const res = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  expect(res.pickedPackage?.version).toBe('1.0.0')

  // The returned/cached meta is normalized: non-install fields dropped, install
  // fields kept, so resolution is unchanged.
  const cachedVersion = res.meta.versions['1.0.0'] as unknown as Record<string, unknown>
  expect(cachedVersion.scripts).toBeUndefined()
  expect(cachedVersion.exports).toBeUndefined()
  expect(cachedVersion.description).toBeUndefined()
  expect((res.meta as unknown as Record<string, unknown>).readme).toBeUndefined()
  expect((res.meta as unknown as Record<string, unknown>)._id).toBeUndefined()
  expect(cachedVersion.dependencies).toEqual({ bar: '^1.0.0' })
  expect(cachedVersion.dist).toBeDefined()
})
