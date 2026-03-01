import { StoreIndex } from '@pnpm/store-index'
import path from 'path'
import { temporaryDirectory } from 'tempy'

test('StoreIndex round-trips data', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StoreIndex(storeDir)
  try {
    const indexDir = path.join(storeDir, 'index')
    const key = path.join(indexDir, 'ab', 'test-entry.mpk')
    expect(idx.get(key)).toBeUndefined()

    const data = { algo: 'sha512', files: new Map([['index.js', { digest: 'abc', size: 100, mode: 0o644 }]]) }
    idx.set(key, data)

    const result = idx.get(key) as typeof data
    expect(result).toBeDefined()
    expect(result.algo).toBe('sha512')
    expect(result.files.get('index.js')?.digest).toBe('abc')

    expect(idx.delete(key)).toBe(true)
    expect(idx.get(key)).toBeUndefined()
  } finally {
    idx.close()
  }
})

test('StoreIndex entries() iterates all entries', () => {
  const storeDir = path.join(temporaryDirectory(), 'store', 'v11')
  const idx = new StoreIndex(storeDir)
  try {
    const indexDir = path.join(storeDir, 'index')
    idx.set(path.join(indexDir, 'ab', 'entry1.mpk'), { a: 1 })
    idx.set(path.join(indexDir, 'cd', 'entry2.mpk'), { b: 2 })

    const entries = [...idx.entries()]
    expect(entries).toHaveLength(2)
  } finally {
    idx.close()
  }
})
