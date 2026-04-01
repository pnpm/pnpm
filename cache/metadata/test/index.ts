import { closeAllMetadataCaches, MetadataCache } from '@pnpm/cache.metadata'
import { temporaryDirectory } from 'tempy'

afterEach(() => {
  closeAllMetadataCaches()
})

test('queueWrite and getIndex', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('is-positive', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': {} },
    modified: '2017-08-17T19:26:00.508Z',
  }, {
    etag: '"abc123"',
    cachedAt: Date.now(),
  })
  db.flush()

  const index = db.getIndex('is-positive')
  expect(index).not.toBeNull()
  expect(index!.etag).toBe('"abc123"')
  expect(index!.modified).toBe('2017-08-17T19:26:00.508Z')
  expect(JSON.parse(index!.distTags)).toEqual({ latest: '1.0.0' })
  expect(JSON.parse(index!.versions)).toEqual({ '1.0.0': {} })
})

test('getHeaders returns only headers without parsing data', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('is-positive', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': {} },
    modified: '2020-01-01T00:00:00.000Z',
  }, {
    etag: '"xyz"',
    cachedAt: Date.now(),
  })
  db.flush()

  const headers = db.getHeaders('is-positive')
  expect(headers).toEqual({
    etag: '"xyz"',
    modified: '2020-01-01T00:00:00.000Z',
  })
})

test('getManifest returns the stored manifest for a version', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('foo', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': { version: '1.0.0', name: 'foo' },
    },
  }, {
    cachedAt: Date.now(),
  })
  db.flush()

  const manifest = db.getManifest('foo', '1.0.0', 'abbreviated')
  expect(manifest).not.toBeNull()
  expect(JSON.parse(manifest!)).toMatchObject({ version: '1.0.0', name: 'foo' })
})

test('getManifest falls back from requested type to full', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('foo', 'full', {
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': { version: '1.0.0', name: 'foo', scripts: { test: 'jest' } },
    },
    time: { '1.0.0': '2020-01-01' },
  }, {
    etag: '"full-etag"',
    cachedAt: Date.now(),
  })
  db.flush()

  // Request abbreviated — should fall back to the full manifest
  const manifest = db.getManifest('foo', '1.0.0', 'abbreviated')
  expect(manifest).not.toBeNull()
  expect(JSON.parse(manifest!).scripts).toBeDefined()

  // Index should also be available
  const index = db.getIndex('foo')
  expect(index).not.toBeNull()
  expect(index!.etag).toBe('"full-etag"')
})

test('delete removes all data for a package', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('pkg', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': {} },
  }, { cachedAt: Date.now() })
  db.flush()

  expect(db.delete('pkg')).toBe(true)
  expect(db.getIndex('pkg')).toBeNull()
  expect(db.getManifest('pkg', '1.0.0', 'abbreviated')).toBeNull()
})

test('listNames returns distinct package names', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('a', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': {} },
  }, { cachedAt: Date.now() })
  db.queueWrite('b', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': {} },
  }, { cachedAt: Date.now() })
  db.flush()

  const names = db.listNames()
  expect(names.sort()).toEqual(['a', 'b'])
})

test('updateCachedAt changes only the timestamp', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  db.queueWrite('pkg', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': {} },
  }, {
    etag: '"e"',
    cachedAt: 1000,
  })
  db.flush()

  db.updateCachedAt('pkg', 2000)

  const index = db.getIndex('pkg')
  expect(index!.cachedAt).toBe(2000)
  expect(index!.etag).toBe('"e"')
})

test('persists across close and reopen', () => {
  const cacheDir = temporaryDirectory()
  const db1 = new MetadataCache(cacheDir)
  db1.queueWrite('persist', 'abbreviated', {
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': { version: '1.0.0' } },
  }, { cachedAt: 1 })
  db1.close()

  const db2 = new MetadataCache(cacheDir)
  const index = db2.getIndex('persist')
  expect(index).not.toBeNull()
  expect(JSON.parse(index!.versions)).toHaveProperty(['1.0.0'])
  db2.close()
})

test('returns null for missing package', () => {
  const cacheDir = temporaryDirectory()
  const db = new MetadataCache(cacheDir)

  expect(db.getIndex('nonexistent')).toBeNull()
  expect(db.getHeaders('nonexistent')).toBeUndefined()
  expect(db.getManifest('nonexistent', '1.0.0', 'abbreviated')).toBeNull()
})
