import type { PackageMeta } from '@pnpm/resolving.registry.types'

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

export interface MemoizeFetchMetadataOptions {
  /**
   * Applied to a settled result's `meta` before the entry is retained for the
   * rest of the resolution phase, so a full document doesn't stay pinned here
   * at full size. Callers awaiting the in-flight request still receive the
   * original document.
   */
  condenseSettledMeta?: (meta: PackageMeta) => PackageMeta
}

/**
 * Memoizes metadata fetches for the whole resolution phase (cleared via
 * `clear`, see `clearResolutionCache`), deduplicating concurrent and repeat
 * requests for the same package.
 *
 * Unlike plain memoization, the entry is swapped for a body-less clone once
 * the request settles. `jsonText` — the raw registry response body, up to tens
 * of MB for a popular package — reaches every caller sharing the in-flight
 * request, so a package resolved by many workspace projects at once mirrors
 * that one body to disk instead of each project separately re-serializing
 * `meta`. Retaining bodies past settlement would pin hundreds of MB on large
 * cold-cache graphs, so a later cache-hit caller that writes the mirror falls
 * back to `JSON.stringify(meta)` in `prepareJsonForDisk`, which is equivalent
 * on read: `loadMeta` re-derives `etag` from the headers line.
 *
 * Because that swap lands a turn after the request settles, both settlement
 * paths write back only while the entry is still their own promise — a `clear`
 * (or a retry that already replaced the entry) must not be undone by a request
 * that was in flight when it happened.
 *
 * A rejected fetch is evicted so a transient network failure is retried by
 * the next request instead of being cached for the rest of the phase.
 */
export function memoizeFetchMetadata (fetch: FetchMetadata, memoOpts?: MemoizeFetchMetadataOptions): MemoizedFetchMetadata {
  const condense = memoOpts?.condenseSettledMeta
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
          cache.set(key, Promise.resolve(settledEntry(result)))
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

  // Runs inside a fire-and-forget then-callback where a throw would become an
  // unhandled rejection, so a failed condense falls back to the uncondensed
  // meta; the resolution path condenses the same document with proper error
  // propagation.
  function settledEntry (result: FetchMetadataResult | FetchMetadataNotModifiedResult): FetchMetadataResult | FetchMetadataNotModifiedResult {
    if (result.notModified) return result
    let meta = result.meta
    if (condense != null) {
      try {
        meta = condense(result.meta)
      } catch {}
    }
    return { ...result, jsonText: undefined, meta }
  }
}
