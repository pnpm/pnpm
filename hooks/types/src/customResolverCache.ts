import type { CustomResolver, WantedDependency } from './index.js'

// Shared cache for canResolve results to avoid calling expensive async operations twice
// WeakMap ensures automatic garbage collection when custom resolvers are no longer referenced
const customResolverCanResolveCache = new WeakMap<CustomResolver, Map<string, boolean>>()

export function getCustomResolverCacheKey (wantedDependency: WantedDependency): string {
  const alias = wantedDependency.alias ?? ''
  const bareSpecifier = wantedDependency.bareSpecifier ?? ''
  return `${alias}@${bareSpecifier}`
}

export function getCachedCanResolve (customResolver: CustomResolver, cacheKey: string): boolean | undefined {
  return customResolverCanResolveCache.get(customResolver)?.get(cacheKey)
}

export function setCachedCanResolve (customResolver: CustomResolver, cacheKey: string, value: boolean): void {
  let cache = customResolverCanResolveCache.get(customResolver)
  if (!cache) {
    cache = new Map<string, boolean>()
    customResolverCanResolveCache.set(customResolver, cache)
  }
  cache.set(cacheKey, value)
}

/**
 * Check if a custom resolver can resolve a wanted dependency, using cache when available
 * This centralizes the cache check/call/store logic
 */
export async function checkCustomResolverCanResolve (
  customResolver: CustomResolver,
  wantedDependency: WantedDependency
): Promise<boolean> {
  if (!customResolver.canResolve) return false

  const cacheKey = getCustomResolverCacheKey(wantedDependency)

  // Check cache first
  const cached = getCachedCanResolve(customResolver, cacheKey)
  if (cached !== undefined) return cached

  // Call canResolve and handle sync/async (await works for both)
  const canResolve = await customResolver.canResolve(wantedDependency)

  // Cache the result
  setCachedCanResolve(customResolver, cacheKey, canResolve)

  return canResolve
}
