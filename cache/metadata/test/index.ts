import { closeAllMetadataCaches, MetadataCache } from '@pnpm/cache.metadata'
import { temporaryDirectory } from 'tempy'

afterEach(() => {
  closeAllMetadataCaches()
})

const sampleMeta = {
  'dist-tags': { latest: '1.0.0' },
  versions: { '1.0.0': { version: '1.0.0', name: 'foo', dist: { tarball: 'https://example.com/foo-1.0.0.tgz', shasum: 'abc' } } },
  modified: '2017-08-17T19:26:00.508Z',
}

test('queueWrite and getIndex', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueWrite('is-positive', sampleMeta, JSON.stringify(sampleMeta), {
    etag: '"abc123"',
    cachedAt: Date.now(),
  })
  db.flush()

  const index = db.getIndex('is-positive')
  expect(index).not.toBeNull()
  expect(index!.etag).toBe('"abc123"')
  expect(index!.modified).toBe('2017-08-17T19:26:00.508Z')
  expect(JSON.parse(index!.distTags)).toEqual({ latest: '1.0.0' })
  expect(JSON.parse(index!.versions)).toHaveProperty(['1.0.0'])
})

test('getHeaders returns only headers', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueWrite('is-positive', sampleMeta, JSON.stringify(sampleMeta), {
    etag: '"xyz"',
    cachedAt: Date.now(),
  })
  db.flush()

  const headers = db.getHeaders('is-positive')
  expect(headers).toEqual({
    etag: '"xyz"',
    modified: '2017-08-17T19:26:00.508Z',
  })
})

test('getBlob returns the raw JSON', () => {
  const db = new MetadataCache(temporaryDirectory())
  const rawJson = JSON.stringify(sampleMeta)

  db.queueWrite('foo', sampleMeta, rawJson, { cachedAt: Date.now() })
  db.flush()

  const blob = db.getBlob('foo')
  expect(blob).toBe(rawJson)
})

test('delete removes all data for a package', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueWrite('pkg', sampleMeta, JSON.stringify(sampleMeta), { cachedAt: Date.now() })
  db.flush()

  expect(db.delete('pkg')).toBe(true)
  expect(db.getIndex('pkg')).toBeNull()
  expect(db.getBlob('pkg')).toBeNull()
})

test('listNames returns distinct package names', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueWrite('a', sampleMeta, '{}', { cachedAt: Date.now() })
  db.queueWrite('b', sampleMeta, '{}', { cachedAt: Date.now() })
  db.flush()

  expect(db.listNames().sort()).toEqual(['a', 'b'])
})

test('updateCachedAt changes only the timestamp', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueWrite('pkg', sampleMeta, '{}', { etag: '"e"', cachedAt: 1000 })
  db.flush()

  db.updateCachedAt('pkg', 2000)

  const index = db.getIndex('pkg')
  expect(index!.cachedAt).toBe(2000)
  expect(index!.etag).toBe('"e"')
})

test('persists across close and reopen', () => {
  const cacheDir = temporaryDirectory()
  const db1 = new MetadataCache(cacheDir)
  db1.queueWrite('persist', sampleMeta, JSON.stringify(sampleMeta), { cachedAt: 1 })
  db1.close()

  const db2 = new MetadataCache(cacheDir)
  expect(db2.getIndex('persist')).not.toBeNull()
  expect(db2.getBlob('persist')).not.toBeNull()
  db2.close()
})

test('returns null for missing package', () => {
  const db = new MetadataCache(temporaryDirectory())

  expect(db.getIndex('nonexistent')).toBeNull()
  expect(db.getHeaders('nonexistent')).toBeUndefined()
  expect(db.getBlob('nonexistent')).toBeNull()
})

test('pending writes are visible to reads', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueWrite('pending', sampleMeta, JSON.stringify(sampleMeta), {
    etag: '"pend"',
    cachedAt: 42,
  })
  // No flush — reads should see pending data
  expect(db.getIndex('pending')).not.toBeNull()
  expect(db.getHeaders('pending')?.etag).toBe('"pend"')
  expect(db.getBlob('pending')).not.toBeNull()
})
