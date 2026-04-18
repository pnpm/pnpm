import { createJsonParseCache } from '../src/jsonCache.js'
import { parseJsonBufferSync } from '../src/parseJson.js'

describe('createJsonParseCache', () => {
  it('creates a working cache with get and set methods', () => {
    const cache = createJsonParseCache()
    expect(cache.get).toBeInstanceOf(Function)
    expect(cache.set).toBeInstanceOf(Function)

    cache.set('key1', { name: 'foo' })
    const result = cache.get('key1') as Record<string, string>
    expect(result.name).toBe('foo')
  })

  it('returns undefined for missing keys', () => {
    const cache = createJsonParseCache()
    expect(cache.get('nonexistent')).toBeUndefined()
  })

  it('evicts oldest entries when maxEntries is exceeded', () => {
    const cache = createJsonParseCache(2)
    cache.set('a', { name: 'a' })
    cache.set('b', { name: 'b' })
    cache.set('c', { name: 'c' }) // should evict 'a'

    expect(cache.get('a')).toBeUndefined()
    expect(cache.get('b')).toBeDefined()
    expect(cache.get('c')).toBeDefined()
  })
})

describe('parseJsonBufferSync', () => {
  it('parses JSON without cache', () => {
    const buffer = Buffer.from('{"name":"foo","version":"1.0.0"}')
    const result = parseJsonBufferSync(buffer) as Record<string, string>
    expect(result.name).toBe('foo')
    expect(result.version).toBe('1.0.0')
  })

  it('uses cache on repeated calls with same digest', () => {
    const cache = createJsonParseCache()
    const buffer1 = Buffer.from('{"name":"foo","version":"1.0.0"}')
    const result1 = parseJsonBufferSync(buffer1, cache, 'abc123') as Record<string, string>
    expect(result1.name).toBe('foo')

    // Parse a different buffer with the same digest — should return cached result
    const buffer2 = Buffer.from('{"name":"bar","version":"2.0.0"}')
    const result2 = parseJsonBufferSync(buffer2, cache, 'abc123') as Record<string, string>
    expect(result2.name).toBe('foo')
    expect(result2.version).toBe('1.0.0')
  })

  it('cache misses when digest differs', () => {
    const cache = createJsonParseCache()
    const buffer1 = Buffer.from('{"name":"foo","version":"1.0.0"}')
    const result1 = parseJsonBufferSync(buffer1, cache, 'digest1') as Record<string, string>
    expect(result1.name).toBe('foo')

    const buffer2 = Buffer.from('{"name":"bar","version":"2.0.0"}')
    const result2 = parseJsonBufferSync(buffer2, cache, 'digest2') as Record<string, string>
    expect(result2.name).toBe('bar')
    expect(result2.version).toBe('2.0.0')
  })

  it('handles BOM in JSON', () => {
    const cache = createJsonParseCache()
    const buffer = Buffer.concat([Buffer.from([0xEF, 0xBB, 0xBF]), Buffer.from('{"name":"bom"}')])
    const result = parseJsonBufferSync(buffer, cache, 'bom1') as Record<string, string>
    expect(result.name).toBe('bom')
  })

  it('caches BOM-stripped result', () => {
    const cache = createJsonParseCache()
    const buffer1 = Buffer.concat([Buffer.from([0xEF, 0xBB, 0xBF]), Buffer.from('{"name":"bom"}')])
    const result1 = parseJsonBufferSync(buffer1, cache, 'bom-digest') as Record<string, string>
    expect(result1.name).toBe('bom')

    // Same digest but different buffer content should return cached
    const buffer2 = Buffer.from('{"name":"other"}')
    const result2 = parseJsonBufferSync(buffer2, cache, 'bom-digest') as Record<string, string>
    expect(result2.name).toBe('bom')
  })
})