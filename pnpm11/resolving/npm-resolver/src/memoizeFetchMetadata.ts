import type {
  FetchMetadataNotModifiedResult,
  FetchMetadataOptions,
  FetchMetadataResult,
} from './fetch.js'

export type FetchMetadata = (pkgName: string, opts: FetchMetadataOptions) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>

export interface MemoizedFetchMetadata {
  fetch: FetchMetadata
  clear: () => void
}

/**
 * Memoizes metadata fetches for the whole resolution phase (cleared via
 * `clear`, see `clearResolutionCache`), deduplicating concurrent and repeat
 * requests for the same package.
 *
 * Unlike a plain memoizer, the cache holds a body-less clone of each result:
 * `jsonText` — the raw registry response body, up to tens of MB for a popular
 * package — reaches only the caller that initiated the fetch, which is the
 * caller that writes the disk mirror. A phase-long cache that kept the bodies
 * would pin hundreds of MB on large cold-cache graphs. A cache-hit caller
 * that also writes the mirror falls back to `JSON.stringify(meta)` in
 * `prepareJsonForDisk`, which is equivalent on read: `loadMeta` re-derives
 * `etag` from the headers line.
 *
 * A rejected fetch is evicted so a transient network failure is retried by
 * the next request instead of being cached for the rest of the phase.
 */
export function memoizeFetchMetadata (fetch: FetchMetadata): MemoizedFetchMetadata {
  const cache = new Map<string, ReturnType<FetchMetadata>>()
  return {
    fetch: (pkgName, opts) => {
      const key = JSON.stringify([pkgName, opts])
      const cached = cache.get(key)
      if (cached != null) return cached
      const pending = fetch(pkgName, opts)
      const bodiless = pending.then((result) =>
        result.notModified ? result : { ...result, jsonText: undefined }
      )
      bodiless.catch(() => cache.delete(key))
      cache.set(key, bodiless)
      return pending
    },
    clear: () => {
      cache.clear()
    },
  }
}
