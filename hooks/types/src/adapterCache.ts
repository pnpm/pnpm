import { type Adapter, type PackageDescriptor } from './index.js'

// Shared cache for canResolve results to avoid calling expensive async operations twice
// WeakMap ensures automatic garbage collection when adapters are no longer referenced
const adapterCanResolveCache = new WeakMap<Adapter, Map<string, boolean>>()

export function getAdapterCacheKey (descriptor: { name: string, range: string }): string {
  return `${descriptor.name}@${descriptor.range}`
}

export function getCachedCanResolve (adapter: Adapter, cacheKey: string): boolean | undefined {
  return adapterCanResolveCache.get(adapter)?.get(cacheKey)
}

export function setCachedCanResolve (adapter: Adapter, cacheKey: string, value: boolean): void {
  let cache = adapterCanResolveCache.get(adapter)
  if (!cache) {
    cache = new Map<string, boolean>()
    adapterCanResolveCache.set(adapter, cache)
  }
  cache.set(cacheKey, value)
}

/**
 * Check if an adapter can resolve a descriptor, using cache when available
 * This centralizes the cache check/call/store logic
 */
export async function checkAdapterCanResolve (
  adapter: Adapter,
  descriptor: PackageDescriptor
): Promise<boolean> {
  if (!adapter.canResolve) return false

  const cacheKey = getAdapterCacheKey(descriptor)

  // Check cache first
  const cached = getCachedCanResolve(adapter, cacheKey)
  if (cached !== undefined) return cached

  // Call canResolve and handle sync/async (await works for both)
  const canResolve = await adapter.canResolve(descriptor)

  // Cache the result
  setCachedCanResolve(adapter, cacheKey, canResolve)

  return canResolve
}
