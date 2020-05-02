import PnpmError from '@pnpm/error'
import {
  Cafs,
  DeferredManifestPromise,
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import { globalWarn } from '@pnpm/logger'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')
import fs = require('mz/fs')
import path = require('path')
import ssri = require('ssri')
import createDownloader, { DownloadFunction } from './createDownloader'

export default function (
  opts: {
    registry: string,
    rawConfig: object,
    alwaysAuth?: boolean,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl?: boolean,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
    userAgent?: string,
    offline?: boolean,
  },
): { tarball: FetchFunction } {
  const download = createDownloader({
    alwaysAuth: opts.alwaysAuth || false,
    ca: opts.ca,
    cert: opts.cert,
    key: opts.key,
    localAddress: opts.localAddress,
    proxy: opts.httpsProxy || opts.proxy,
    registry: opts.registry,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    // TODO: cover with tests this option
    // https://github.com/pnpm/pnpm/issues/1062
    strictSSL: typeof opts.strictSsl === 'boolean'
      ? opts.strictSsl
      : true,
    userAgent: opts.userAgent,
  })
  const getCreds = getCredentialsByURI.bind(null, opts.rawConfig)
  return {
    tarball: fetchFromTarball.bind(null, {
      fetchFromRemoteTarball: fetchFromRemoteTarball.bind(null, {
        download,
        getCredentialsByURI: mem((registry: string) => getCreds(registry)),
        offline: opts.offline,
      }),
    }) as FetchFunction,
  }
}

function fetchFromTarball (
  ctx: {
    fetchFromRemoteTarball: (
      cafs: Cafs,
      dist: {
        integrity?: string,
        registry?: string,
        tarball: string,
      },
      opts: FetchOptions,
    ) => Promise<FetchResult>,
  },
  cafs: Cafs,
  resolution: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  opts: FetchOptions,
) {
  if (resolution.tarball.startsWith('file:')) {
    const tarball = path.join(opts.lockfileDir, resolution.tarball.slice(5))
    return fetchFromLocalTarball(cafs, tarball, {
      integrity: resolution.integrity,
      manifest: opts.manifest,
    })
  }
  return ctx.fetchFromRemoteTarball(cafs, resolution, opts)
}

async function fetchFromRemoteTarball (
  ctx: {
    offline?: boolean,
    download: DownloadFunction,
    getCredentialsByURI: (registry: string) => {
      authHeaderValue: string | undefined,
      alwaysAuth: boolean | undefined,
    },
  },
  cafs: Cafs,
  dist: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  opts: FetchOptions,
) {
  try {
    return await fetchFromLocalTarball(cafs, opts.cachedTarballLocation, {
      integrity: dist.integrity,
      manifest: opts.manifest,
    })
  } catch (err) {
    // ignore errors for missing files or broken/partial archives
    switch (err.code) {
      case 'Z_BUF_ERROR':
        if (ctx.offline) {
          throw new PnpmError(
            'CORRUPTED_TARBALL',
            `The cached tarball at "${opts.cachedTarballLocation}" is corrupted. Cannot redownload it as offline mode was requested.`,
          )
        }
        globalWarn(`Redownloading corrupted cached tarball: ${opts.cachedTarballLocation}`)
        break
      case 'EINTEGRITY':
        if (ctx.offline) {
          throw new PnpmError(
            'BAD_TARBALL_CHECKSUM',
            `The cached tarball at "${opts.cachedTarballLocation}" did not pass the integrity check. Cannot redownload it as offline mode was requested.`,
          )
        }
        globalWarn(`The cached tarball at "${opts.cachedTarballLocation}" did not pass the integrity check. Redownloading.`)
        break
      case 'ENOENT':
        if (ctx.offline) {
          throw new PnpmError('NO_OFFLINE_TARBALL', `Could not find ${opts.cachedTarballLocation} in local registry mirror`)
        }
        break
      default:
        throw err
    }

    const auth = dist.registry ? ctx.getCredentialsByURI(dist.registry) : undefined
    return ctx.download(dist.tarball, opts.cachedTarballLocation, {
      auth,
      cafs,
      integrity: dist.integrity,
      manifest: opts.manifest,
      onProgress: opts.onProgress,
      onStart: opts.onStart,
      registry: dist.registry,
    })
  }
}

async function fetchFromLocalTarball (
  cafs: Cafs,
  tarball: string,
  opts: {
    integrity?: string,
    manifest?: DeferredManifestPromise,
  },
): Promise<FetchResult> {
  try {
    const tarballStream = fs.createReadStream(tarball)
    const [fetchResult] = (
      await Promise.all([
        cafs.addFilesFromTarball(tarballStream, opts.manifest),
        opts.integrity && (ssri.checkStream(tarballStream, opts.integrity) as any), // tslint:disable-line
      ])
    )
    return { filesIndex: fetchResult }
  } catch (err) {
    err.attempts = 1
    err.resource = tarball
    throw err
  }
}
