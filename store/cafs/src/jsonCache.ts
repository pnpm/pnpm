import { LRUCache } from 'lru-cache'

export interface JsonParseCache {
  get: (digest: string) => unknown | undefined
  set: (digest: string, value: unknown) => void
}

const DEFAULT_MAX_ENTRIES = 5000

export function createJsonParseCache (maxEntries: number = DEFAULT_MAX_ENTRIES): JsonParseCache {
  const cache = new LRUCache<string, object>({
    max: maxEntries,
  })
  return {
    get: (digest) => cache.get(digest),
    set: (digest, value) => cache.set(digest, value as object),
  }
}
