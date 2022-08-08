import PnpmError from '@pnpm/error'
import {
  Cafs,
  FetchFunction,
  FetchOptions,
} from '@pnpm/fetcher-base'
import {
  FetchFromRegistry,
  GetCredentials,
  RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import createDownloader, {
  DownloadFunction,
  TarballIntegrityError,
} from './remoteTarballFetcher'
import createLocalTarballFetcher from './localTarballFetcher'
import createGitHostedTarballFetcher, { waitForFilesIndex } from './gitHostedTarballFetcher'

export { BadTarballError } from './errorTypes'

export { TarballIntegrityError, waitForFilesIndex }

export default function (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  opts: {
    timeout?: number
    retry?: RetryTimeoutOptions
    offline?: boolean
  }
): { localTarball: FetchFunction, remoteTarball: FetchFunction, gitHostedTarball: FetchFunction } {
  const download = createDownloader(fetchFromRegistry, {
    retry: opts.retry,
    timeout: opts.timeout,
  })

  const remoteTarballFetcher = fetchFromTarball.bind(null, {
    download,
    getCredentialsByURI: getCredentials,
    offline: opts.offline,
  }) as FetchFunction

  return {
    localTarball: createLocalTarballFetcher(),
    remoteTarball: remoteTarballFetcher,
    gitHostedTarball: createGitHostedTarballFetcher(remoteTarballFetcher),
  }
}

async function fetchFromTarball (
  ctx: {
    download: DownloadFunction
    getCredentialsByURI: (registry: string) => {
      authHeaderValue: string | undefined
      alwaysAuth: boolean | undefined
    }
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
    throw new PnpmError('NO_OFFLINE_TARBALL',
      `A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from ${resolution.tarball}.`)
  }
  const auth = resolution.registry ? ctx.getCredentialsByURI(resolution.registry) : undefined
  return ctx.download(resolution.tarball, {
    auth,
    cafs,
    integrity: resolution.integrity,
    manifest: opts.manifest,
    onProgress: opts.onProgress,
    onStart: opts.onStart,
    registry: resolution.registry,
  })
}
