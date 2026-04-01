import { closeAllMetadataCaches, MetadataCache } from '@pnpm/cache.metadata'
import { temporaryDirectory } from 'tempy'

afterEach(() => {
  closeAllMetadataCaches()
})

test('set and get metadata', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('is-positive', 'abbreviated', '{"name":"is-positive","versions":{}}', {
    etag: '"abc123"',
    modified: '2017-08-17T19:26:00.508Z',
    cachedAt: Date.now(),
  })

  const row = db.get('is-positive', 'abbreviated')
  expect(row).not.toBeNull()
  expect(row!.etag).toBe('"abc123"')
  expect(row!.modified).toBe('2017-08-17T19:26:00.508Z')
  expect(JSON.parse(row!.data).name).toBe('is-positive')
})

test('getHeaders returns only headers without parsing data', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('is-positive', 'abbreviated', '{"name":"is-positive"}', {
    etag: '"xyz"',
    modified: '2020-01-01T00:00:00.000Z',
    cachedAt: Date.now(),
  })

  const headers = db.getHeaders('is-positive', 'abbreviated')
  expect(headers).toEqual({
    etag: '"xyz"',
    modified: '2020-01-01T00:00:00.000Z',
  })
})

test('abbreviated falls back to full-filtered then full', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('foo', 'full', '{"name":"foo","time":{"1.0.0":"2020-01-01"}}', {
    etag: '"full-etag"',
    cachedAt: Date.now(),
  })

  // Request abbreviated — should get the full row
  const row = db.get('foo', 'abbreviated')
  expect(row).not.toBeNull()
  expect(row!.etag).toBe('"full-etag"')
  expect(JSON.parse(row!.data).time).toBeDefined()

  const headers = db.getHeaders('foo', 'abbreviated')
  expect(headers?.etag).toBe('"full-etag"')
})

test('abbreviated prefers full-filtered over full', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('bar', 'full', '{"name":"bar"}', { etag: '"full"', cachedAt: Date.now() })
  db.set('bar', 'full-filtered', '{"name":"bar"}', { etag: '"filtered"', cachedAt: Date.now() })

  const row = db.get('bar', 'abbreviated')
  expect(row!.etag).toBe('"filtered"')
})

test('delete removes all types for a package', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('pkg', 'abbreviated', '{}', { cachedAt: Date.now() })
  db.set('pkg', 'full', '{}', { cachedAt: Date.now() })

  expect(db.delete('pkg')).toBe(true)
  expect(db.get('pkg', 'abbreviated')).toBeNull()
  expect(db.get('pkg', 'full')).toBeNull()
})

test('listNames returns distinct package names', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('a', 'abbreviated', '{}', { cachedAt: Date.now() })
  db.set('a', 'full', '{}', { cachedAt: Date.now() })
  db.set('b', 'abbreviated', '{}', { cachedAt: Date.now() })

  const names = db.listNames()
  expect(names.sort()).toEqual(['a', 'b'])
})

test('listNames filters by type', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('a', 'abbreviated', '{}', { cachedAt: Date.now() })
  db.set('b', 'full', '{}', { cachedAt: Date.now() })

  expect(db.listNames('abbreviated')).toEqual(['a'])
  expect(db.listNames('full')).toEqual(['b'])
})

test('updateCachedAt changes only the timestamp', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.set('pkg', 'abbreviated', '{"original":true}', {
    etag: '"e"',
    cachedAt: 1000,
  })

  db.updateCachedAt('pkg', 'abbreviated', 2000)

  const row = db.get('pkg', 'abbreviated')
  expect(row!.cachedAt).toBe(2000)
  expect(row!.etag).toBe('"e"')
  expect(JSON.parse(row!.data).original).toBe(true)
})

test('persists across close and reopen', () => {
  const cacheDir = temporaryDirectory()
  const db1 = new MetadataCache(cacheDir)
  db1.set('persist', 'abbreviated', '{"v":1}', { cachedAt: 1 })
  db1.close()

  const db2 = new MetadataCache(cacheDir)
  const row = db2.get('persist', 'abbreviated')
  expect(row).not.toBeNull()
  expect(JSON.parse(row!.data).v).toBe(1)
  db2.close()
})

test('returns null for missing package', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  expect(db.get('nonexistent', 'abbreviated')).toBeNull()
  expect(db.getHeaders('nonexistent', 'abbreviated')).toBeUndefined()
})
