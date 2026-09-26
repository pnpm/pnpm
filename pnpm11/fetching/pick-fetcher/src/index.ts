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
import type { CustomFetcher, CustomFetcherDelegation } from '@pnpm/hooks.types'
import { type AtomicResolution, classifyResolution, type Resolution } from '@pnpm/resolving.resolver-base'
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
    const lockedIntegrity = getLockedArchiveIntegrity(resolution)
    const fetchers = lockedIntegrity == null
      ? fetcherByHostingType
      : bindArchiveIntegrity(fetcherByHostingType, lockedIntegrity)
    for (const customFetcher of opts.customFetchers) {
      if (customFetcher.canFetch && customFetcher.fetch) {
        // eslint-disable-next-line no-await-in-loop
        const canFetch = await callWithLockedIntegrity(resolution, lockedIntegrity, () => customFetcher.canFetch!(opts.packageId, resolution))

        if (canFetch) {
          // Preserve `this` for custom fetchers that implement their optional
          // resolution contract as a method.
          const resolutionNeedsFetch = typeof customFetcher.resolutionNeedsFetch === 'function'
            ? customFetcher.resolutionNeedsFetch.bind(customFetcher)
            : undefined
          return Object.assign(
            async (cafs: Cafs, resolution: AtomicResolution, fetchOpts: FetchOptions): Promise<FetchResult> => {
              const result = await callWithLockedIntegrity(resolution, lockedIntegrity, () => customFetcher.fetch!(cafs, resolution, fetchOpts, fetchers))
              if (isCustomFetcherDelegation(result)) {
                const delegate = (lockedIntegrity == null
                  ? result.delegate
                  : preserveArchiveIntegrity(result.delegate, lockedIntegrity)) as AtomicResolution
                const fetch = pickBuiltinFetcher(fetcherByHostingType, delegate) as FetchFunction
                return fetch(cafs, delegate, fetchOpts)
              }
              return result
            },
            { resolutionNeedsFetch }
          ) as FetchFunction
        }
      }
    }
    return pickBuiltinFetcher(fetchers, lockedIntegrity == null
      ? resolution
      : preserveArchiveIntegrity(resolution, lockedIntegrity))
  }

  return pickBuiltinFetcher(fetcherByHostingType, resolution)
}

function getLockedArchiveIntegrity (resolution: AtomicResolution): string | undefined {
  if (resolution.type != null && resolution.type !== 'binary') return
  return typeof resolution.integrity === 'string' && resolution.integrity.length > 0
    ? resolution.integrity
    : undefined
}

async function callWithLockedIntegrity<Result> (resolution: Resolution, integrity: string | undefined, call: () => Result | Promise<Result>): Promise<Result> {
  try {
    return await call()
  } finally {
    if (integrity != null && (!('integrity' in resolution) || resolution.integrity !== integrity)) {
      Object.assign(resolution, { integrity })
    }
  }
}

function bindArchiveIntegrity (fetchers: Fetchers, integrity: string): Fetchers {
  return {
    localTarball: bindFetcherIntegrity(fetchers.localTarball, integrity),
    remoteTarball: bindFetcherIntegrity(fetchers.remoteTarball, integrity),
    gitHostedTarball: bindFetcherIntegrity(fetchers.gitHostedTarball, integrity),
    directory: async () => rejectArchiveDelegation('directory'),
    git: async () => rejectArchiveDelegation('git'),
    binary: bindFetcherIntegrity(fetchers.binary, integrity),
  }
}

function bindFetcherIntegrity<FetcherResolution extends Resolution, Options, Result> (
  fetch: FetchFunction<FetcherResolution, Options, Result>,
  integrity: string
): FetchFunction<FetcherResolution, Options, Result> {
  return Object.assign(
    (cafs: Cafs, resolution: FetcherResolution, opts: Options) => fetch(cafs, preserveArchiveIntegrity(resolution, integrity), opts),
    { resolutionNeedsFetch: fetch.resolutionNeedsFetch?.bind(fetch) }
  )
}

function preserveArchiveIntegrity<FetcherResolution extends Resolution> (resolution: FetcherResolution, integrity: string): FetcherResolution {
  if (resolution.type != null && resolution.type !== 'binary') {
    return rejectArchiveDelegation(resolution.type)
  }
  return { ...resolution, integrity }
}

function rejectArchiveDelegation (resolutionType: string): never {
  throw new PnpmError('TARBALL_INTEGRITY', `Cannot verify the locked archive integrity after delegating to resolution type "${resolutionType}".`)
}

function isCustomFetcherDelegation (result: FetchResult | CustomFetcherDelegation): result is CustomFetcherDelegation {
  return result != null && typeof result === 'object' && 'delegate' in result && !('filesMap' in result)
}

function pickBuiltinFetcher (fetcherByHostingType: Fetchers, resolution: AtomicResolution): PickedFetcher {
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
