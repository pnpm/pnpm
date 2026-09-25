import fs from 'node:fs'
import path from 'node:path'
import type { DatabaseSync } from 'node:sqlite'
import util from 'node:util'

import { expect, test } from '@jest/globals'
import { packForStorage, ReadOnlyStoreIndex, StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { temporaryDirectory } from 'tempy'

test('StoreIndex round-trips data via SQLite key', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StoreIndex(storeDir)
  try {
    const key = storeIndexKey('sha512-abc123', 'lodash@4.17.21')
    expect(idx.get(key)).toBeUndefined()

    const data = { algo: 'sha512', files: new Map([['index.js', { digest: 'abc', size: 100, mode: 0o644 }]]) }
    idx.set(key, data)

    const result = idx.get(key) as typeof data
    expect(result).toBeDefined()
    expect(result.algo).toBe('sha512')
    expect(result.files.get('index.js')?.digest).toBe('abc')

    expect(idx.has(key)).toBe(true)
    expect(idx.delete(key)).toBe(true)
    expect(idx.get(key)).toBeUndefined()
    expect(idx.has(key)).toBe(false)
  } finally {
    idx.close()
  }
})

test('StoreIndex entries() iterates all SQLite entries', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StoreIndex(storeDir)
  try {
    const key1 = storeIndexKey('sha512-aaa', 'pkg-a@1.0.0')
    const key2 = storeIndexKey('sha512-bbb', 'pkg-b@2.0.0')
    idx.set(key1, { a: 1 })
    idx.set(key2, { b: 2 })

    const entries = [...idx.entries()]
    expect(entries).toHaveLength(2)
    const keys = entries.map(([k]) => k)
    expect(keys).toContain(key1)
    expect(keys).toContain(key2)
  } finally {
    idx.close()
  }
})

test('StoreIndex update mutates an existing row without creating a missing row', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StoreIndex(storeDir)
  try {
    idx.set('present', { values: new Map([['one', 1]]) })
    expect(idx.update('present', (value) => {
      const row = value as { values: Map<string, number> }
      row.values.set('two', 2)
      return row
    })).toBe(true)
    expect(idx.get('present')).toEqual({ values: new Map([['one', 1], ['two', 2]]) })
    expect(idx.update('missing', () => ({ created: true }))).toBe(false)
    expect(idx.get('missing')).toBeUndefined()
  } finally {
    idx.close()
  }
})

test('StoreIndex update reports a rollback failure without hiding the update error', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StoreIndex(storeDir)
  idx.set('present', { value: 1 })
  let thrown: unknown
  try {
    idx.update('present', (value) => {
      idx.close()
      return value
    })
  } catch (err: unknown) {
    thrown = err
  }
  expect(util.types.isNativeError(thrown)).toBe(true)
  if (!util.types.isNativeError(thrown) || !('errors' in thrown) || !Array.isArray(thrown.errors)) {
    throw new Error('Expected update and rollback errors to be aggregated')
  }
  expect(thrown.name).toBe('AggregateError')
  expect(thrown.message).toContain('present')
  expect(thrown.errors).toHaveLength(2)
})

// The immutable open only works on a runtime that honors the immutable URI;
// this is purely a Node-version property, independent of platform.
const supportsImmutableUri = nodeSupportsImmutableSqliteUri()
// chmod 0555 has no effect on Windows (and `?` is illegal in filenames there),
// so the read-only-directory tests below cannot hold on win32.
const canAssertReadonlyDir = process.platform !== 'win32'
const testFrozenOpen = (canAssertReadonlyDir && supportsImmutableUri) ? test : test.skip

testFrozenOpen('StoreIndex frozen mode reads a WAL db on a read-only directory and refuses writes', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const key = storeIndexKey('sha512-frozen', 'frozen-pkg@1.0.0')
  const data = { algo: 'sha512', files: new Map([['index.js', { digest: 'abc', size: 100, mode: 0o644 }]]) }

  // Seed the WAL db while the directory is still writable, then close
  // the connection so the on-disk file is a settled WAL db — what a
  // read-only-store seed-build would leave behind.
  const seed = new StoreIndex(storeDir)
  seed.set(key, data)
  seed.close()

  // Drop the store dir to read + execute only: no writes permitted, so
  // SQLite cannot create any -shm / -wal sidecar.
  fs.chmodSync(storeDir, 0o555)
  try {
    const idx = new ReadOnlyStoreIndex(storeDir)
    try {
      const result = idx.get(key) as typeof data
      expect(result).toBeDefined()
      expect(result.algo).toBe('sha512')
      expect(result.files.get('index.js')?.digest).toBe('abc')
      expect(idx.has(key)).toBe(true)

      expect(() => {
        idx.set(key, data)
      }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_FROZEN_STORE_WRITE' }))
      expect(() => {
        idx.delete(key)
      }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_FROZEN_STORE_WRITE' }))
      expect(() => {
        idx.update(key, value => value)
      }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_FROZEN_STORE_WRITE' }))

      // The immutable open must not create any sidecar under the
      // read-only directory.
      for (const sidecar of ['index.db-shm', 'index.db-wal', 'index.db-journal']) {
        expect(fs.existsSync(path.join(storeDir, sidecar))).toBe(false)
      }
    } finally {
      idx.close()
    }
  } finally {
    // Restore write permission so the tempdir can be cleaned up.
    fs.chmodSync(storeDir, 0o755)
  }
})

// `?` is a legal filename character on POSIX but a SQLite URI delimiter, so a
// raw `file:${path}?immutable=1` would truncate the path here. (`?` is illegal
// in Windows filenames, so this case cannot arise there.)
testFrozenOpen('StoreIndex frozen mode opens under a store path containing a "?"', () => {
  const storeDir = path.join(temporaryDirectory(), 'weird?store', 'v11')
  const key = storeIndexKey('sha512-q', 'q-pkg@1.0.0')
  const data = { algo: 'sha512', files: new Map([['index.js', { digest: 'q', size: 1, mode: 0o644 }]]) }

  const seed = new StoreIndex(storeDir)
  seed.set(key, data)
  seed.close()

  const idx = new ReadOnlyStoreIndex(storeDir)
  try {
    expect(idx.has(key)).toBe(true)
    expect((idx.get(key) as typeof data).algo).toBe('sha512')
  } finally {
    idx.close()
  }
})

// On a runtime that cannot honor the immutable URI, a frozen open must fail
// fast with actionable guidance rather than SQLite's cryptic "unable to open
// database file". This is keyed only off the Node version (the error is
// platform-independent), so it runs on Windows too when the runtime is old.
const testUnsupportedNode = supportsImmutableUri ? test.skip : test

testUnsupportedNode('StoreIndex frozen mode refuses to open on a Node.js without immutable-URI support', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  expect(() => new ReadOnlyStoreIndex(storeDir))
    .toThrow(expect.objectContaining({ code: 'ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE' }))
})

// The `immutable=1` URI open only works on Node.js that passes
// SQLITE_OPEN_URI to SQLite: v22.15.0+, v23.11.0+, and every v24+. On older
// runtimes (including pnpm's `engines` floor of 22.13) the open throws a clear
// ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE instead — asserted separately below.
function nodeSupportsImmutableSqliteUri (): boolean {
  const [major, minor] = process.versions.node.split('.', 2).map(Number)
  if (major < 22) return false
  if (major === 22) return minor >= 15
  if (major === 23) return minor >= 11
  return true
}

test('StoreIndex keeps using DatabaseSync.exec when the host provides it', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new TracingPrepareStoreIndex(storeDir)
  try {
    const preparedSql = preparedSqlOf(idx)
    expect(preparedSql.some(sql => /^\s*(?:pragma|create table)/i.test(sql))).toBe(false)
    expect(preparedSql).toHaveLength(6)
    expect(fs.existsSync(path.join(storeDir, 'index.fallback'))).toBe(false)
  } finally {
    idx.close()
  }
})

test('StoreIndex runs SQL through prepared statements when DatabaseSync.exec is missing', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new MissingExecStoreIndex(storeDir)
  try {
    const preparedSql = preparedSqlOf(idx)
    expect(preparedSql.some(sql => /^\s*pragma/i.test(sql))).toBe(true)
    expect(preparedSql.some(sql => /create table/i.test(sql))).toBe(true)
    const key = storeIndexKey('sha512-abc', 'pkg@1.0.0')
    idx.set(key, { n: 1 })
    idx.setRawMany([
      { key, buffer: packForStorage({ n: 2 }) },
      { key: 'other', buffer: packForStorage({ n: 3 }) },
    ])
    expect(idx.get(key)).toEqual({ n: 2 })
    expect(idx.update('other', (value) => ({ n: (value as { n: number }).n + 1 }))).toBe(true)
    expect(idx.get('other')).toEqual({ n: 4 })
    idx.deleteMany([key, 'other'])
    expect(idx.has(key)).toBe(false)
    idx.checkpoint()
    expect(fs.existsSync(path.join(storeDir, 'index.db'))).toBe(true)
    expect(fs.existsSync(path.join(storeDir, 'index.fallback'))).toBe(false)
  } finally {
    idx.close()
  }
})

test('StoreIndex falls back to a file when node:sqlite cannot prepare statements', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const key = storeIndexKey('sha512-file', 'pkg@1.0.0')
  const data = { algo: 'sha512', files: new Map([['index.js', { digest: 'abc', size: 100, mode: 0o644 }]]) }
  const writer = new IncompleteSqliteStoreIndex(storeDir)
  const reader = new IncompleteSqliteStoreIndex(storeDir)
  try {
    writer.set(key, data)
    expect(reader.get(key)).toEqual(data)
    writer.setRawMany([
      { key: 'a', buffer: packForStorage({ a: 1 }) },
      { key: 'b', buffer: packForStorage({ b: 2 }) },
    ])
    expect(reader.get('a')).toEqual({ a: 1 })
    expect([...reader.keys()]).toEqual(expect.arrayContaining([key, 'a', 'b']))
    expect([...reader.entries()].map(([entryKey]) => entryKey)).toEqual(expect.arrayContaining([key, 'a', 'b']))
    expect(writer.update('missing', () => ({ created: true }))).toBe(false)
    expect(writer.update('a', (value) => ({ a: (value as { a: number }).a + 1 }))).toBe(true)
    expect(reader.get('a')).toEqual({ a: 2 })
    expect(() => {
      writer.update('a', () => {
        throw new Error('boom')
      })
    }).toThrow('boom')
    expect(reader.get('a')).toEqual({ a: 2 })
    expect(writer.delete(key)).toBe(true)
    expect(reader.has(key)).toBe(false)
    expect(writer.delete('missing')).toBe(false)
    writer.deleteMany(['a', 'b'])
    expect(reader.has('a')).toBe(false)
    writer.checkpoint()
    expect(fs.existsSync(path.join(storeDir, 'index.fallback'))).toBe(true)
  } finally {
    writer.close()
    reader.close()
  }
})

test('StoreIndex falls back to a file when prepared statements cannot run', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StatementRunMissingStoreIndex(storeDir)
  try {
    idx.set('k', { n: 1 })
    expect(idx.get('k')).toEqual({ n: 1 })
    expect(fs.existsSync(path.join(storeDir, 'index.fallback'))).toBe(true)
  } finally {
    idx.close()
  }
})

// openConnection runs inside the base constructor, before subclass fields exist.
const preparedSqlByIndex = new WeakMap<StoreIndex, string[]>()

function preparedSqlOf (idx: StoreIndex): string[] {
  const preparedSql = preparedSqlByIndex.get(idx)
  if (preparedSql == null) {
    throw new Error('Missing prepared SQL trace')
  }
  return preparedSql
}

class TracingPrepareStoreIndex extends StoreIndex {
  protected override openConnection (storeDir: string): DatabaseSync {
    return tracePrepare(this, super.openConnection(storeDir))
  }
}

class MissingExecStoreIndex extends StoreIndex {
  protected override openConnection (storeDir: string): DatabaseSync {
    const db = tracePrepare(this, super.openConnection(storeDir))
    ;(db as { exec?: unknown }).exec = undefined
    return db
  }
}

function tracePrepare (idx: StoreIndex, db: DatabaseSync): DatabaseSync {
  const preparedSql: string[] = []
  preparedSqlByIndex.set(idx, preparedSql)
  const prepare = db.prepare.bind(db)
  db.prepare = (sql: string) => {
    preparedSql.push(sql)
    return prepare(sql)
  }
  return db
}

class IncompleteSqliteStoreIndex extends StoreIndex {
  protected override openConnection (_storeDir: string): DatabaseSync {
    return {
      close () {},
    } as unknown as DatabaseSync
  }
}

class StatementRunMissingStoreIndex extends StoreIndex {
  protected override openConnection (_storeDir: string): DatabaseSync {
    return {
      prepare () {
        return {
          run () {
            throw new TypeError('stmt.run is not a function')
          },
        }
      },
      close () {},
    } as unknown as DatabaseSync
  }
}
