import { PnpmError } from '@pnpm/error'
import type {
  BinaryFetcher,
  DirectoryFetcher,
  Fetchers,
  FetchFunction,
  FetchOptions,
  FetchResult,
  GitFetcher,
} from '@pnpm/fetching.fetcher-base'
import type { CustomFetcher } from '@pnpm/hooks.types'
import { type AtomicResolution, classifyResolution } from '@pnpm/resolving.resolver-base'
import type { Cafs } from '@pnpm/store.cafs-types'

export type PickedFetcher = FetchFunction | DirectoryFetcher | GitFetcher | BinaryFetcher

export async function pickFetcher (
  fetcherByHostingType: Fetchers,
  resolution: AtomicResolution,
  opts?: {
    customFetchers?: CustomFetcher[]
    packageId: string
  }
): Promise<PickedFetcher> {
  // Try custom fetcher hooks first if available
  // Custom fetchers act as complete fetcher replacements
  if (opts?.customFetchers && opts.customFetchers.length > 0) {
    for (const customFetcher of opts.customFetchers) {
      if (customFetcher.canFetch && customFetcher.fetch) {
        // eslint-disable-next-line no-await-in-loop
        const canFetch = await customFetcher.canFetch(opts.packageId, resolution)

        if (canFetch) {
          // Preserve `this` for custom fetchers that implement their optional
          // resolution contract as a method.
          const resolutionNeedsFetch = typeof customFetcher.resolutionNeedsFetch === 'function'
            ? customFetcher.resolutionNeedsFetch.bind(customFetcher)
            : undefined
          return Object.assign(
            async (cafs: Cafs, resolution: AtomicResolution, fetchOpts: FetchOptions): Promise<FetchResult> =>
              customFetcher.fetch!(cafs, resolution, fetchOpts, fetcherByHostingType),
            { resolutionNeedsFetch }
          ) as FetchFunction
        }
      }
    }
  }

  const fetcherType = classifyResolution(resolution)
  if (fetcherType === 'custom') {
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
