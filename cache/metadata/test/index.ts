import { closeAllMetadataCaches, MetadataCache } from '@pnpm/cache.metadata'
import { temporaryDirectory } from 'tempy'

afterEach(() => {
  closeAllMetadataCaches()
})

const sampleData = '{"dist-tags":{"latest":"1.0.0"},"versions":{"1.0.0":{"version":"1.0.0"}},"modified":"2017-08-17T19:26:00.508Z"}'

test('queueSet and get', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('is-positive', sampleData, {
    etag: '"abc123"',
    modified: '2017-08-17T19:26:00.508Z',
    cachedAt: Date.now(),
  })
  db.flush()

  const row = db.get('is-positive')
  expect(row).not.toBeNull()
  expect(row!.etag).toBe('"abc123"')
  expect(row!.modified).toBe('2017-08-17T19:26:00.508Z')
  expect(row!.data).toBe(sampleData)
  expect(row!.isFull).toBe(false)
})

test('getHeaders returns only headers', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('is-positive', sampleData, {
    etag: '"xyz"',
    modified: '2020-01-01T00:00:00.000Z',
    cachedAt: Date.now(),
  })
  db.flush()

  const headers = db.getHeaders('is-positive')
  expect(headers).toEqual({
    etag: '"xyz"',
    modified: '2020-01-01T00:00:00.000Z',
  })
})

test('isFull flag', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('pkg', sampleData, { cachedAt: Date.now(), isFull: true })
  db.flush()

  const row = db.get('pkg')
  expect(row!.isFull).toBe(true)
})

test('delete removes data', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('pkg', sampleData, { cachedAt: Date.now() })
  db.flush()

  expect(db.delete('pkg')).toBe(true)
  expect(db.get('pkg')).toBeNull()
})

test('listNames', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('a', '{}', { cachedAt: Date.now() })
  db.queueSet('b', '{}', { cachedAt: Date.now() })
  db.flush()

  expect(db.listNames().sort()).toEqual(['a', 'b'])
})

test('updateCachedAt', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('pkg', sampleData, { etag: '"e"', cachedAt: 1000 })
  db.flush()

  db.updateCachedAt('pkg', 2000)

  const row = db.get('pkg')
  expect(row!.cachedAt).toBe(2000)
  expect(row!.etag).toBe('"e"')
})

test('persists across close and reopen', () => {
  const cacheDir = temporaryDirectory()
  const db1 = new MetadataCache(cacheDir)
  db1.queueSet('persist', sampleData, { cachedAt: 1 })
  db1.close()

  const db2 = new MetadataCache(cacheDir)
  expect(db2.get('persist')).not.toBeNull()
  db2.close()
})

test('returns null/undefined for missing package', () => {
  const db = new MetadataCache(temporaryDirectory())

  expect(db.get('nonexistent')).toBeNull()
  expect(db.getHeaders('nonexistent')).toBeUndefined()
})

test('pending writes are visible to reads', () => {
  const db = new MetadataCache(temporaryDirectory())

  db.queueSet('pending', sampleData, { etag: '"pend"', cachedAt: 42 })
  // No flush
  expect(db.get('pending')).not.toBeNull()
  expect(db.getHeaders('pending')?.etag).toBe('"pend"')
})
