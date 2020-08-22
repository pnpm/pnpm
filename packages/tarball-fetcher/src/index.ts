import PnpmError from '@pnpm/error'
import {
  Cafs,
  DeferredManifestPromise,
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import {
  FetchFromRegistry,
  GetCredentials,
  RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import createDownloader, { DownloadFunction } from './createDownloader'
import path = require('path')
import fs = require('mz/fs')
import ssri = require('ssri')

export default function (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  opts: {
    retry?: RetryTimeoutOptions,
    offline?: boolean,
  }
): { tarball: FetchFunction } {
  const download = createDownloader(fetchFromRegistry, {
    retry: opts.retry,
  })
  return {
    tarball: fetchFromTarball.bind(null, {
      download,
      getCredentialsByURI: getCredentials,
      offline: opts.offline,
    }) as FetchFunction,
  }
}

function fetchFromTarball (
  ctx: {
    download: DownloadFunction,
    getCredentialsByURI: (registry: string) => {
      authHeaderValue: string | undefined,
      alwaysAuth: boolean | undefined,
    },
    offline?: boolean,
  },
  cafs: Cafs,
  resolution: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  opts: FetchOptions
) {
  if (resolution.tarball.startsWith('file:')) {
    const tarball = resolvePath(opts.lockfileDir, resolution.tarball.slice(5))
    return fetchFromLocalTarball(cafs, tarball, {
      integrity: resolution.integrity,
      manifest: opts.manifest,
    })
  }
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

const isAbsolutePath = /^[/]|^[A-Za-z]:/

function resolvePath (where: string, spec: string) {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

async function fetchFromLocalTarball (
  cafs: Cafs,
  tarball: string,
  opts: {
    integrity?: string,
    manifest?: DeferredManifestPromise,
  }
): Promise<FetchResult> {
  try {
    const tarballStream = fs.createReadStream(tarball)
    const [fetchResult] = (
      await Promise.all([
        cafs.addFilesFromTarball(tarballStream, opts.manifest),
        opts.integrity && (ssri.checkStream(tarballStream, opts.integrity) as any), // eslint-disable-line
      ])
    )
    return { filesIndex: fetchResult }
  } catch (err) {
    err.attempts = 1
    err.resource = tarball
    throw err
  }
}
