import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { temporaryDirectory } from 'tempy'

import type { FetchMetadataOptions } from '../src/fetch.js'
import { prepareIndexedForDisk } from '../src/mirror.js'
import type { RegistryPackageSpec } from '../src/parseBareSpecifier.js'
import { getPkgMirrorPath, type PackageMetaCache, pickPackage } from '../src/pickPackage.js'

const REGISTRY = 'https://registry.npmjs.org/'

function fooMeta (): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': { latest: '2.0.0' },
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        dist: {
          shasum: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
        },
      },
      '2.0.0': {
        name: 'foo',
        version: '2.0.0',
        dist: {
          shasum: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          tarball: 'https://registry.npmjs.org/foo/-/foo-2.0.0.tgz',
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

// Writes an indexed mirror whose 2.0.0 fragment (the max satisfying version)
// is corrupted in place: spans stay valid, the fragment bytes do not parse.
function writeCorruptMirror (pkgMirror: string): void {
  const meta = fooMeta()
  const content = prepareIndexedForDisk(meta, '"etag-corrupt"')
  const fragmentStart = content.indexOf(Buffer.from(JSON.stringify(meta.versions['2.0.0'])))
  if (fragmentStart === -1) throw new Error('fragment not found in the serialized mirror')
  content.fill('x', fragmentStart, fragmentStart + 10)
  fs.mkdirSync(path.dirname(pkgMirror), { recursive: true })
  fs.writeFileSync(pkgMirror, content)
}

test('offline resolution over a corrupt fragment fails with NO_OFFLINE_META', async () => {
  const cacheDir = temporaryDirectory()
  writeCorruptMirror(getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo'))

  const ctx = {
    fetch: async () => {
      throw new Error('offline resolution must not hit the network')
    },
    metaCache: createMetaCache(),
    cacheDir,
    offline: true,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '>=1.0.0' }

  await expect(
    pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_NO_OFFLINE_META' })
})

test('a corrupt fragment behind a 304 triggers a cache-bypassing refetch that heals the mirror', async () => {
  const cacheDir = temporaryDirectory()
  const pkgMirror = getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, REGISTRY, 'foo')
  writeCorruptMirror(pkgMirror)
  const corruptContent = fs.readFileSync(pkgMirror)

  const meta = fooMeta()
  type CacheBypassFetchMetadataOptions = FetchMetadataOptions & { cacheBypass?: boolean }
  const fetchCalls: CacheBypassFetchMetadataOptions[] = []
  const ctx = {
    fetch: async (pkgName: string, fetchOpts: CacheBypassFetchMetadataOptions) => {
      fetchCalls.push(fetchOpts)
      if (!fetchOpts.cacheBypass) return { notModified: true as const }
      return { meta, jsonText: JSON.stringify(meta), etag: '"fresh"' }
    },
    metaCache: createMetaCache(),
    cacheDir,
  }
  const spec: RegistryPackageSpec = { type: 'range', name: 'foo', fetchSpec: '>=1.0.0' }

  const res = await pickPackage(ctx, spec, { registry: REGISTRY, dryRun: false, preferredVersionSelectors: undefined })
  expect(res.pickedPackage?.version).toBe('2.0.0')
  expect(fetchCalls).toHaveLength(2)
  expect(fetchCalls[0].etag).toBe('"etag-corrupt"')
  expect(fetchCalls[0].cacheBypass).toBeUndefined()
  expect(fetchCalls[1].cacheBypass).toBe(true)

  // The mirror rewrite is fire-and-forget; wait for the poisoned bytes to be
  // replaced by the refetched document.
  /* eslint-disable no-await-in-loop */
  for (let attempt = 0; attempt < 100; attempt++) {
    if (!fs.readFileSync(pkgMirror).equals(corruptContent)) break
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  /* eslint-enable no-await-in-loop */
  expect(fs.readFileSync(pkgMirror).equals(corruptContent)).toBe(false)
})
