import path from 'node:path'

import { expect, test } from '@jest/globals'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
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
