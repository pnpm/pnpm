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
 * Unlike plain memoization, the settled cache holds a body-less clone of each
 * result. `jsonText` — the raw registry response body, up to tens of MB for a
 * popular package — is shared by callers waiting on the same in-flight request,
 * then removed from the settled cache entry. This prevents concurrent callers
 * from independently serializing the same large metadata object while still
 * avoiding phase-long retention of raw response bodies.
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
      cache.set(key, pending)
      void pending.then(
        (result) => {
          if (cache.get(key) !== pending) return
          cache.set(
            key,
            Promise.resolve(
              result.notModified ? result : { ...result, jsonText: undefined }
            )
          )
        },
        () => {
          if (cache.get(key) === pending) cache.delete(key)
        }
      )
      return pending
    },
    clear: () => {
      cache.clear()
    },
  }
}
