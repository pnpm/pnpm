import {
  getCustomResolverCacheKey,
  getCachedCanResolve,
  setCachedCanResolve,
  checkCustomResolverCanResolve,
  type CustomResolver,
} from '../src/index.js'

describe('customResolverCache', () => {
  describe('getCustomResolverCacheKey', () => {
    test('generates cache key from descriptor', () => {
      const wantedDependency = { alias: 'test-package', bareSpecifier: '1.0.0' }
      expect(getCustomResolverCacheKey(wantedDependency)).toBe('test-package@1.0.0')
    })

    test('handles scoped packages', () => {
      const wantedDependency = { alias: '@org/package', bareSpecifier: '^2.0.0' }
      expect(getCustomResolverCacheKey(wantedDependency)).toBe('@org/package@^2.0.0')
    })

    test('handles version ranges', () => {
      const wantedDependency = { alias: 'lodash', bareSpecifier: '>=4.0.0 <5.0.0' }
      expect(getCustomResolverCacheKey(wantedDependency)).toBe('lodash@>=4.0.0 <5.0.0')
    })
  })

  describe('getCachedCanResolve', () => {
    test('returns undefined for uncached custom resolver', () => {
      const customResolver: CustomResolver = {
        canResolve: () => true,
      }
      const result = getCachedCanResolve(customResolver, 'test@1.0.0')
      expect(result).toBeUndefined()
    })

    test('returns cached value when available', () => {
      const customResolver: CustomResolver = {
        canResolve: () => true,
      }
      setCachedCanResolve(customResolver, 'test@1.0.0', true)
      const result = getCachedCanResolve(customResolver, 'test@1.0.0')
      expect(result).toBe(true)
    })

    test('returns false when cached as false', () => {
      const customResolver: CustomResolver = {
        canResolve: () => true,
      }
      setCachedCanResolve(customResolver, 'test@1.0.0', false)
      const result = getCachedCanResolve(customResolver, 'test@1.0.0')
      expect(result).toBe(false)
    })

    test('cache is isolated per custom resolver', () => {
      const customResolver1: CustomResolver = { canResolve: () => true }
      const customResolver2: CustomResolver = { canResolve: () => false }

      setCachedCanResolve(customResolver1, 'pkg@1.0.0', true)
      setCachedCanResolve(customResolver2, 'pkg@1.0.0', false)

      expect(getCachedCanResolve(customResolver1, 'pkg@1.0.0')).toBe(true)
      expect(getCachedCanResolve(customResolver2, 'pkg@1.0.0')).toBe(false)
    })

    test('cache is isolated per descriptor', () => {
      const customResolver: CustomResolver = { canResolve: () => true }

      setCachedCanResolve(customResolver, 'pkg1@1.0.0', true)
      setCachedCanResolve(customResolver, 'pkg2@1.0.0', false)

      expect(getCachedCanResolve(customResolver, 'pkg1@1.0.0')).toBe(true)
      expect(getCachedCanResolve(customResolver, 'pkg2@1.0.0')).toBe(false)
    })
  })

  describe('setCachedCanResolve', () => {
    test('creates new cache for custom resolver', () => {
      const customResolver: CustomResolver = { canResolve: () => true }

      setCachedCanResolve(customResolver, 'test@1.0.0', true)

      expect(getCachedCanResolve(customResolver, 'test@1.0.0')).toBe(true)
    })

    test('updates existing cache entry', () => {
      const customResolver: CustomResolver = { canResolve: () => true }

      setCachedCanResolve(customResolver, 'test@1.0.0', false)
      setCachedCanResolve(customResolver, 'test@1.0.0', true)

      expect(getCachedCanResolve(customResolver, 'test@1.0.0')).toBe(true)
    })

    test('allows multiple cache entries per custom resolver', () => {
      const customResolver: CustomResolver = { canResolve: () => true }

      setCachedCanResolve(customResolver, 'pkg1@1.0.0', true)
      setCachedCanResolve(customResolver, 'pkg2@2.0.0', false)
      setCachedCanResolve(customResolver, 'pkg3@3.0.0', true)

      expect(getCachedCanResolve(customResolver, 'pkg1@1.0.0')).toBe(true)
      expect(getCachedCanResolve(customResolver, 'pkg2@2.0.0')).toBe(false)
      expect(getCachedCanResolve(customResolver, 'pkg3@3.0.0')).toBe(true)
    })
  })

  describe('checkCustomResolverCanResolve', () => {
    test('returns false when custom resolver has no canResolve', async () => {
      const customResolver: CustomResolver = {}
      const wantedDependency = { alias: 'test', bareSpecifier: '1.0.0' }

      const result = await checkCustomResolverCanResolve(customResolver, wantedDependency)

      expect(result).toBe(false)
    })

    test('calls canResolve and caches result (true)', async () => {
      let callCount = 0
      const customResolver: CustomResolver = {
        canResolve: () => {
          callCount++
          return true
        },
      }
      const wantedDependency = { alias: 'test', bareSpecifier: '1.0.0' }

      const result1 = await checkCustomResolverCanResolve(customResolver, wantedDependency)
      const result2 = await checkCustomResolverCanResolve(customResolver, wantedDependency)

      expect(result1).toBe(true)
      expect(result2).toBe(true)
      expect(callCount).toBe(1) // Should only be called once due to caching
    })

    test('calls canResolve and caches result (false)', async () => {
      let callCount = 0
      const customResolver: CustomResolver = {
        canResolve: () => {
          callCount++
          return false
        },
      }
      const wantedDependency = { alias: 'test', bareSpecifier: '1.0.0' }

      const result1 = await checkCustomResolverCanResolve(customResolver, wantedDependency)
      const result2 = await checkCustomResolverCanResolve(customResolver, wantedDependency)

      expect(result1).toBe(false)
      expect(result2).toBe(false)
      expect(callCount).toBe(1) // Should only be called once due to caching
    })

    test('handles async canResolve', async () => {
      let callCount = 0
      const customResolver: CustomResolver = {
        canResolve: async () => {
          callCount++
          await new Promise(resolve => setTimeout(resolve, 10))
          return true
        },
      }
      const wantedDependency = { alias: 'test', bareSpecifier: '1.0.0' }

      const result1 = await checkCustomResolverCanResolve(customResolver, wantedDependency)
      const result2 = await checkCustomResolverCanResolve(customResolver, wantedDependency)

      expect(result1).toBe(true)
      expect(result2).toBe(true)
      expect(callCount).toBe(1) // Should only be called once due to caching
    })

    test('different descriptors are cached separately', async () => {
      let callCount = 0
      const customResolver: CustomResolver = {
        canResolve: (descriptor) => {
          callCount++
          return descriptor.alias === 'match'
        },
      }

      const result1 = await checkCustomResolverCanResolve(customResolver, { alias: 'match', bareSpecifier: '1.0.0' })
      const result2 = await checkCustomResolverCanResolve(customResolver, { alias: 'no-match', bareSpecifier: '1.0.0' })
      const result3 = await checkCustomResolverCanResolve(customResolver, { alias: 'match', bareSpecifier: '1.0.0' })

      expect(result1).toBe(true)
      expect(result2).toBe(false)
      expect(result3).toBe(true)
      expect(callCount).toBe(2) // Called for 'match' and 'no-match', but cached for second 'match'
    })

    test('uses cache key based on alias and bareSpecifier', async () => {
      let callCount = 0
      const customResolver: CustomResolver = {
        canResolve: () => {
          callCount++
          return true
        },
      }

      // Same package, different versions
      await checkCustomResolverCanResolve(customResolver, { alias: 'test', bareSpecifier: '1.0.0' })
      await checkCustomResolverCanResolve(customResolver, { alias: 'test', bareSpecifier: '2.0.0' })

      expect(callCount).toBe(2) // Different bareSpecifiers mean different cache keys
    })
  })
})
