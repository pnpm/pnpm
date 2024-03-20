import '@total-typescript/ts-reset'

import type {
  FetchFromRegistry,
  GetAuthHeader,
  RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { PnpmError } from '@pnpm/error'
import { TarballIntegrityError } from '@pnpm/worker'

import { FetchFunction, Cafs, FetchOptions } from '@pnpm/types'
import { createLocalTarballFetcher } from './localTarballFetcher'
import { createGitHostedTarballFetcher } from './gitHostedTarballFetcher'
import { createDownloader, type DownloadFunction } from './remoteTarballFetcher'

export { TarballIntegrityError }
export { BadTarballError } from './errorTypes'

export type TarballFetchers = {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
}

export function createTarballFetcher(
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: {
    rawConfig: object
    unsafePerm?: boolean | undefined
    ignoreScripts?: boolean | undefined
    timeout?: number | undefined
    retry?: RetryTimeoutOptions | undefined
    offline?: boolean | undefined
  }
): TarballFetchers {
  const download = createDownloader(fetchFromRegistry, {
    retry: opts.retry,
    timeout: opts.timeout,
  })

  const remoteTarballFetcher = fetchFromTarball.bind(null, {
    download,
    getAuthHeaderByURI: getAuthHeader,
    offline: opts.offline,
  }) as FetchFunction

  return {
    localTarball: createLocalTarballFetcher(),
    remoteTarball: remoteTarballFetcher,
    gitHostedTarball: createGitHostedTarballFetcher(remoteTarballFetcher, opts),
  }
}

async function fetchFromTarball(
  ctx: {
    download: DownloadFunction
    getAuthHeaderByURI: (registry: string) => string | undefined
    offline?: boolean
  },
  cafs: Cafs,
  resolution: {
    integrity?: string
    registry?: string
    tarball: string
  },
  opts: FetchOptions
) {
  if (ctx.offline) {
    throw new PnpmError(
      'NO_OFFLINE_TARBALL',
      `A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from ${resolution.tarball}.`
    )
  }

  return ctx.download(resolution.tarball, {
    getAuthHeaderByURI: ctx.getAuthHeaderByURI,
    cafs,
    integrity: resolution.integrity,
    readManifest: opts.readManifest,
    onProgress: opts.onProgress,
    onStart: opts.onStart,
    registry: resolution.registry,
    filesIndexFile: opts.filesIndexFile,
    pkg: opts.pkg,
  })
}
