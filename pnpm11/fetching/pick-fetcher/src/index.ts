import { PnpmError } from '@pnpm/error'
import type { BinaryFetcher, DirectoryFetcher, Fetchers, FetchFunction, FetchOptions, FetchResult, GitFetcher } from '@pnpm/fetching.fetcher-base'
import type { CustomFetcher } from '@pnpm/hooks.types'
import { type AtomicResolution, classifyResolution } from '@pnpm/resolving.resolver-base'
import type { Cafs } from '@pnpm/store.cafs-types'

export async function pickFetcher (
  fetcherByHostingType: Fetchers,
  resolution: AtomicResolution,
  opts?: {
    customFetchers?: CustomFetcher[]
    packageId: string
  }
): Promise<FetchFunction | DirectoryFetcher | GitFetcher | BinaryFetcher> {
  // Try custom fetcher hooks first if available
  // Custom fetchers act as complete fetcher replacements
  if (opts?.customFetchers && opts.customFetchers.length > 0) {
    for (const customFetcher of opts.customFetchers) {
      if (customFetcher.canFetch && customFetcher.fetch) {
        // eslint-disable-next-line no-await-in-loop
        const canFetch = await customFetcher.canFetch(opts.packageId, resolution)

        if (canFetch) {
          // Return a wrapper FetchFunction that calls the custom fetcher's fetch method.
          // The custom fetcher's fetch receives cafs, resolution, opts, and the standard
          // fetchers for delegation. Its optional resolution contract is forwarded so the
          // requester treats it like any standard fetcher (defaults apply when it opts out).
          // The hook is bound to the custom fetcher so an implementation that uses `this`
          // still works once it's copied onto the wrapper.
          return Object.assign(
            async (cafs: Cafs, resolution: AtomicResolution, fetchOpts: FetchOptions): Promise<FetchResult> =>
              customFetcher.fetch!(cafs, resolution, fetchOpts, fetcherByHostingType),
            { resolutionNeedsFetch: customFetcher.resolutionNeedsFetch?.bind(customFetcher) }
          ) as FetchFunction
        }
      }
    }
  }

  // No custom fetcher handled the fetch, use standard fetcher selection.
  const fetcherType = classifyResolution(resolution)
  if (fetcherType === 'custom') {
    // Custom resolution type that wasn't handled by any custom fetcher above.
    throw new PnpmError(
      'UNSUPPORTED_RESOLUTION_TYPE',
      `Cannot fetch dependency with custom resolution type "${resolution.type}". ` +
      'Custom resolutions must be handled by custom fetchers.'
    )
  }

  const fetch = fetcherByHostingType[fetcherType]

  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type ?? 'tarball'}" is not supported`)
  }

  return fetch
}
