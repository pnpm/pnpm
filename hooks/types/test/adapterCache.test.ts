import {
  getAdapterCacheKey,
  getCachedCanResolve,
  setCachedCanResolve,
  checkAdapterCanResolve,
  type Adapter,
} from '../src/index.js'

describe('adapterCache', () => {
  describe('getAdapterCacheKey', () => {
    test('generates cache key from descriptor', () => {
      const descriptor = { name: 'test-package', range: '1.0.0' }
      expect(getAdapterCacheKey(descriptor)).toBe('test-package@1.0.0')
    })

    test('handles scoped packages', () => {
      const descriptor = { name: '@org/package', range: '^2.0.0' }
      expect(getAdapterCacheKey(descriptor)).toBe('@org/package@^2.0.0')
    })

    test('handles version ranges', () => {
      const descriptor = { name: 'lodash', range: '>=4.0.0 <5.0.0' }
      expect(getAdapterCacheKey(descriptor)).toBe('lodash@>=4.0.0 <5.0.0')
    })
  })

  describe('getCachedCanResolve', () => {
    test('returns undefined for uncached adapter', () => {
      const adapter: Adapter = {
        canResolve: () => true,
      }
      const result = getCachedCanResolve(adapter, 'test@1.0.0')
      expect(result).toBeUndefined()
    })

    test('returns cached value when available', () => {
      const adapter: Adapter = {
        canResolve: () => true,
      }
      setCachedCanResolve(adapter, 'test@1.0.0', true)
      const result = getCachedCanResolve(adapter, 'test@1.0.0')
      expect(result).toBe(true)
    })

    test('returns false when cached as false', () => {
      const adapter: Adapter = {
        canResolve: () => true,
      }
      setCachedCanResolve(adapter, 'test@1.0.0', false)
      const result = getCachedCanResolve(adapter, 'test@1.0.0')
      expect(result).toBe(false)
    })

    test('cache is isolated per adapter', () => {
      const adapter1: Adapter = { canResolve: () => true }
      const adapter2: Adapter = { canResolve: () => false }

      setCachedCanResolve(adapter1, 'pkg@1.0.0', true)
      setCachedCanResolve(adapter2, 'pkg@1.0.0', false)

      expect(getCachedCanResolve(adapter1, 'pkg@1.0.0')).toBe(true)
      expect(getCachedCanResolve(adapter2, 'pkg@1.0.0')).toBe(false)
    })

    test('cache is isolated per descriptor', () => {
      const adapter: Adapter = { canResolve: () => true }

      setCachedCanResolve(adapter, 'pkg1@1.0.0', true)
      setCachedCanResolve(adapter, 'pkg2@1.0.0', false)

      expect(getCachedCanResolve(adapter, 'pkg1@1.0.0')).toBe(true)
      expect(getCachedCanResolve(adapter, 'pkg2@1.0.0')).toBe(false)
    })
  })

  describe('setCachedCanResolve', () => {
    test('creates new cache for adapter', () => {
      const adapter: Adapter = { canResolve: () => true }

      setCachedCanResolve(adapter, 'test@1.0.0', true)

      expect(getCachedCanResolve(adapter, 'test@1.0.0')).toBe(true)
    })

    test('updates existing cache entry', () => {
      const adapter: Adapter = { canResolve: () => true }

      setCachedCanResolve(adapter, 'test@1.0.0', false)
      setCachedCanResolve(adapter, 'test@1.0.0', true)

      expect(getCachedCanResolve(adapter, 'test@1.0.0')).toBe(true)
    })

    test('allows multiple cache entries per adapter', () => {
      const adapter: Adapter = { canResolve: () => true }

      setCachedCanResolve(adapter, 'pkg1@1.0.0', true)
      setCachedCanResolve(adapter, 'pkg2@2.0.0', false)
      setCachedCanResolve(adapter, 'pkg3@3.0.0', true)

      expect(getCachedCanResolve(adapter, 'pkg1@1.0.0')).toBe(true)
      expect(getCachedCanResolve(adapter, 'pkg2@2.0.0')).toBe(false)
      expect(getCachedCanResolve(adapter, 'pkg3@3.0.0')).toBe(true)
    })
  })

  describe('checkAdapterCanResolve', () => {
    test('returns false when adapter has no canResolve', async () => {
      const adapter: Adapter = {}
      const descriptor = { name: 'test', range: '1.0.0' }

      const result = await checkAdapterCanResolve(adapter, descriptor)

      expect(result).toBe(false)
    })

    test('calls canResolve and caches result (true)', async () => {
      let callCount = 0
      const adapter: Adapter = {
        canResolve: () => {
          callCount++
          return true
        },
      }
      const descriptor = { name: 'test', range: '1.0.0' }

      const result1 = await checkAdapterCanResolve(adapter, descriptor)
      const result2 = await checkAdapterCanResolve(adapter, descriptor)

      expect(result1).toBe(true)
      expect(result2).toBe(true)
      expect(callCount).toBe(1) // Should only be called once due to caching
    })

    test('calls canResolve and caches result (false)', async () => {
      let callCount = 0
      const adapter: Adapter = {
        canResolve: () => {
          callCount++
          return false
        },
      }
      const descriptor = { name: 'test', range: '1.0.0' }

      const result1 = await checkAdapterCanResolve(adapter, descriptor)
      const result2 = await checkAdapterCanResolve(adapter, descriptor)

      expect(result1).toBe(false)
      expect(result2).toBe(false)
      expect(callCount).toBe(1) // Should only be called once due to caching
    })

    test('handles async canResolve', async () => {
      let callCount = 0
      const adapter: Adapter = {
        canResolve: async () => {
          callCount++
          await new Promise(resolve => setTimeout(resolve, 10))
          return true
        },
      }
      const descriptor = { name: 'test', range: '1.0.0' }

      const result1 = await checkAdapterCanResolve(adapter, descriptor)
      const result2 = await checkAdapterCanResolve(adapter, descriptor)

      expect(result1).toBe(true)
      expect(result2).toBe(true)
      expect(callCount).toBe(1) // Should only be called once due to caching
    })

    test('different descriptors are cached separately', async () => {
      let callCount = 0
      const adapter: Adapter = {
        canResolve: (descriptor) => {
          callCount++
          return descriptor.name === 'match'
        },
      }

      const result1 = await checkAdapterCanResolve(adapter, { name: 'match', range: '1.0.0' })
      const result2 = await checkAdapterCanResolve(adapter, { name: 'no-match', range: '1.0.0' })
      const result3 = await checkAdapterCanResolve(adapter, { name: 'match', range: '1.0.0' })

      expect(result1).toBe(true)
      expect(result2).toBe(false)
      expect(result3).toBe(true)
      expect(callCount).toBe(2) // Called for 'match' and 'no-match', but cached for second 'match'
    })

    test('uses cache key based on name and range', async () => {
      let callCount = 0
      const adapter: Adapter = {
        canResolve: () => {
          callCount++
          return true
        },
      }

      // Same package, different versions
      await checkAdapterCanResolve(adapter, { name: 'test', range: '1.0.0' })
      await checkAdapterCanResolve(adapter, { name: 'test', range: '2.0.0' })

      expect(callCount).toBe(2) // Different ranges mean different cache keys
    })
  })
})
