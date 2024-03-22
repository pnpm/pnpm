import '@total-typescript/ts-reset'

import { PnpmError } from '@pnpm/error'
import { TarballIntegrityError } from '@pnpm/worker'

import type {
  Cafs,
  FetchResult,
  FetchOptions,
  GetAuthHeader,
  DownloadFunction,
  FetchFromRegistry,
  DependencyManifest,
  RetryTimeoutOptions,
} from '@pnpm/types'

import { createDownloader } from './remoteTarballFetcher.js'
import { createLocalTarballFetcher } from './localTarballFetcher.js'
import { createGitHostedTarballFetcher } from './gitHostedTarballFetcher.js'

export { TarballIntegrityError }
export { BadTarballError } from './errorTypes/index.js'

export type TarballFetchers = {
  localTarball: (cafs: Cafs, resolution: {
    integrity?: string | undefined
    registry?: string | undefined
    tarball: string
  }, opts: FetchOptions) => Promise<{
    filesIndex: Record<string, string>;
    manifest: DependencyManifest;
  }>
  remoteTarball: (cafs: Cafs, resolution: {
    tarball: string;
    integrity: string | undefined;
    registry: string | undefined;
  }, opts: FetchOptions) => Promise<FetchResult>
  gitHostedTarball: (cafs: Cafs, resolution: {
    tarball: string;
    integrity?: string | undefined;
    registry?: string | undefined;
  }, opts: FetchOptions) => Promise<{
    filesIndex: Record<string, string>;
    manifest: DependencyManifest | undefined;
  }>
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
  })

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
    offline?: boolean | undefined
  },
  cafs: Cafs,
  resolution: {
    tarball: string
    integrity?: string | undefined
    registry?: string | undefined
  },
  opts: FetchOptions
): Promise<FetchResult> {
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
