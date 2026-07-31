import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import { temporaryDirectory } from 'tempy'

import { clearMeta } from '../src/clearMeta.js'
import {
  isMalformedMirrorFragmentError,
  loadMeta,
  loadMetaHeaders,
  prepareIndexedForDisk,
  prepareJsonForDisk,
  saveMeta,
} from '../src/mirror.js'

function fixtureMeta (): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': { latest: '2.0.0' },
    versions: {
      '1.0.0': {
        name: 'foo',
        version: '1.0.0',
        dist: { integrity: 'sha512-aaa', shasum: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', tarball: 'https://registry.example/foo/-/foo-1.0.0.tgz' },
        dependencies: { bar: '^1.0.0' },
        description: 'not part of the abbreviated field set',
      } as PackageInRegistry,
      '2.0.0': {
        name: 'foo',
        version: '2.0.0',
        dist: { integrity: 'sha512-bbb', shasum: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', tarball: 'https://registry.example/foo/-/foo-2.0.0.tgz' },
      } as PackageInRegistry,
    },
    time: {
      '1.0.0': '2020-01-01T00:00:00.000Z',
      '2.0.0': '2021-01-01T00:00:00.000Z',
      modified: '2021-01-01T00:00:00.000Z',
    },
    modified: '2021-01-01T00:00:00.000Z',
  }
}

test('indexed mirror round-trips through save and load', async () => {
  const pkgMirror = path.join(temporaryDirectory(), 'foo.jsonl')
  const meta = fixtureMeta()
  await saveMeta(pkgMirror, prepareIndexedForDisk(meta, '"etag-1"'))

  const loaded = await loadMeta(pkgMirror)
  expect(loaded).not.toBeNull()
  expect(loaded!.name).toBe('foo')
  expect(loaded!['dist-tags']).toStrictEqual({ latest: '2.0.0' })
  expect(loaded!.etag).toBe('"etag-1"')
  expect(loaded!.modified).toBe('2021-01-01T00:00:00.000Z')
  expect(loaded!.time).toStrictEqual(meta.time)
  expect(Object.keys(loaded!.versions)).toStrictEqual(['1.0.0', '2.0.0'])
  expect(loaded!.versions['1.0.0']).toStrictEqual(meta.versions['1.0.0'])
  expect(loaded!.versions['2.0.0']).toStrictEqual(meta.versions['2.0.0'])
})

test('hydrated manifests keep one identity and accept mutation', async () => {
  const pkgMirror = path.join(temporaryDirectory(), 'foo.jsonl')
  await saveMeta(pkgMirror, prepareIndexedForDisk(fixtureMeta(), undefined))

  const loaded = (await loadMeta(pkgMirror))!
  const first = loaded.versions['1.0.0']
  expect(loaded.versions['1.0.0']).toBe(first)
  first.name = 'renamed'
  expect(loaded.versions['1.0.0'].name).toBe('renamed')

  const descriptor = Object.getOwnPropertyDescriptor(loaded.versions, '1.0.0')!
  const copy: PackageMeta['versions'] = {}
  Object.defineProperty(copy, '1.0.0', descriptor)
  expect(copy['1.0.0']).toBe(first)
})

test('condensing load strips manifests to the abbreviated field set and marks the document', async () => {
  const pkgMirror = path.join(temporaryDirectory(), 'foo.jsonl')
  await saveMeta(pkgMirror, prepareIndexedForDisk(fixtureMeta(), undefined))

  const loaded = (await loadMeta(pkgMirror, { condense: true }))!
  expect(clearMeta(loaded)).toBe(loaded)
  expect(loaded.versions['1.0.0'].description).toBeUndefined()
  expect(loaded.versions['1.0.0'].dependencies).toStrictEqual({ bar: '^1.0.0' })
})

test('NDJSON mirror still loads', async () => {
  const pkgMirror = path.join(temporaryDirectory(), 'foo.jsonl')
  const meta = fixtureMeta()
  await saveMeta(pkgMirror, prepareJsonForDisk(meta, '"etag-2"'))

  const loaded = await loadMeta(pkgMirror)
  expect(loaded).not.toBeNull()
  expect(loaded!.etag).toBe('"etag-2"')
  expect(loaded!.versions['1.0.0']).toStrictEqual(meta.versions['1.0.0'])
})

test('a truncated indexed mirror reads as a cache miss', async () => {
  const pkgMirror = path.join(temporaryDirectory(), 'foo.jsonl')
  const content = prepareIndexedForDisk(fixtureMeta(), undefined)
  fs.mkdirSync(path.dirname(pkgMirror), { recursive: true })
  fs.writeFileSync(pkgMirror, content.subarray(0, content.length - 10))

  expect(await loadMeta(pkgMirror)).toBeNull()
})

test('loadMetaHeaders reads both layouts', async () => {
  const dir = temporaryDirectory()
  const indexed = path.join(dir, 'indexed.jsonl')
  const ndjson = path.join(dir, 'ndjson.jsonl')
  await saveMeta(indexed, prepareIndexedForDisk(fixtureMeta(), '"etag-3"'))
  await saveMeta(ndjson, prepareJsonForDisk(fixtureMeta(), '"etag-4"'))

  expect(await loadMetaHeaders(indexed)).toStrictEqual({ etag: '"etag-3"', modified: '2021-01-01T00:00:00.000Z' })
  expect(await loadMetaHeaders(ndjson)).toStrictEqual({ etag: '"etag-4"', modified: '2021-01-01T00:00:00.000Z' })
})

test('a corrupt fragment throws on access without breaking the rest of the document', async () => {
  const pkgMirror = path.join(temporaryDirectory(), 'foo.jsonl')
  const meta = fixtureMeta()
  const content = prepareIndexedForDisk(meta, undefined)
  // Corrupt the bytes of the 1.0.0 fragment in place, keeping the file length
  // and the index spans intact.
  const fragmentStart = content.indexOf(Buffer.from(JSON.stringify(meta.versions['1.0.0'])))
  expect(fragmentStart).toBeGreaterThan(-1)
  content.fill('x', fragmentStart, fragmentStart + 10)
  fs.mkdirSync(path.dirname(pkgMirror), { recursive: true })
  fs.writeFileSync(pkgMirror, content)

  const loaded = await loadMeta(pkgMirror)
  expect(loaded).not.toBeNull()
  expect(Object.keys(loaded!.versions)).toStrictEqual(['1.0.0', '2.0.0'])
  let thrown: unknown
  try {
    void loaded!.versions['1.0.0']
  } catch (err: unknown) {
    thrown = err
  }
  expect(isMalformedMirrorFragmentError(thrown)).toBe(true)
  expect(loaded!.versions['2.0.0']).toStrictEqual(meta.versions['2.0.0'])
})
