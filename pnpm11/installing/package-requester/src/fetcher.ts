import type {
  Fetchers,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetching.fetcher-base'
import { type PickedFetcher, pickFetcher } from '@pnpm/fetching.pick-fetcher'
import type { CustomFetcher } from '@pnpm/hooks.types'
import { logger } from '@pnpm/logger'
import type { AtomicResolution } from '@pnpm/resolving.resolver-base'
import type { Cafs } from '@pnpm/store.cafs-types'

export const packageRequestLogger = logger('package-requester')

export async function fetcher (
  fetcherByHostingType: Fetchers,
  cafs: Cafs,
  customFetchers: CustomFetcher[] | undefined,
  packageId: string,
  resolution: AtomicResolution,
  opts: FetchOptions,
  pickedFetcher?: PickedFetcher
): Promise<FetchResult> {
  try {
    const fetch = pickedFetcher ?? await pickFetcher(fetcherByHostingType, resolution, {
      customFetchers,
      packageId,
    })
    const result = await fetch(cafs, resolution as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any -- the picked fetcher is a union of fetchers, so its resolution parameter narrows to never
    return result
  } catch (err: any) { // eslint-disable-line
    packageRequestLogger.warn({
      message: `Fetching ${packageId} failed!`,
      prefix: opts.lockfileDir,
    })
    throw err
  }
}
