import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { ReadOnlyStoreIndex, StoreIndex, storeIndexKey } from '@pnpm/store.index'
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
