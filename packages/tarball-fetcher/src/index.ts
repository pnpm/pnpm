import PnpmError from '@pnpm/error'
import {
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import { globalWarn } from '@pnpm/logger'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')
import fs = require('mz/fs')
import path = require('path')
import pathTemp = require('path-temp')
import rimraf = require('rimraf')
import ssri = require('ssri')
import * as unpackStream from 'unpack-stream'
import createDownloader, { DownloadFunction } from './createDownloader'

export type IgnoreFunction = (filename: string) => boolean

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
    ignoreFile?: IgnoreFunction,
    offline?: boolean,
    fsIsCaseSensitive?: boolean,
  },
): { tarball: FetchFunction } {
  const download = createDownloader({
    alwaysAuth: opts.alwaysAuth || false,
    ca: opts.ca,
    cert: opts.cert,
    fsIsCaseSensitive: typeof opts.fsIsCaseSensitive === 'boolean'
      ? opts.fsIsCaseSensitive
      : false,
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
        ignoreFile: opts.ignoreFile,
        offline: opts.offline,
      }),
      ignore: opts.ignoreFile,
    }) as FetchFunction,
  }
}

function fetchFromTarball (
  ctx: {
    fetchFromRemoteTarball: (
      dir: string,
      dist: {
        integrity?: string,
        registry?: string,
        tarball: string,
      },
      opts: FetchOptions,
    ) => Promise<FetchResult>,
    ignore?: IgnoreFunction,
  },
  resolution: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  target: string,
  opts: FetchOptions,
) {
  if (resolution.tarball.startsWith('file:')) {
    const tarball = path.join(opts.lockfileDir, resolution.tarball.slice(5))
    return fetchFromLocalTarball(tarball, target, {
      ignore: ctx.ignore,
      integrity: resolution.integrity,
    })
  }
  return ctx.fetchFromRemoteTarball(target, resolution, opts)
}

async function fetchFromRemoteTarball (
  ctx: {
    offline?: boolean,
    download: DownloadFunction,
    ignoreFile?: IgnoreFunction,
    getCredentialsByURI: (registry: string) => {
      authHeaderValue: string | undefined,
      alwaysAuth: boolean | undefined,
    },
  },
  unpackTo: string,
  dist: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  opts: FetchOptions,
) {
  try {
    return await fetchFromLocalTarball(opts.cachedTarballLocation, unpackTo, {
      integrity: dist.integrity,
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
      ignore: ctx.ignoreFile,
      integrity: dist.integrity,
      onProgress: opts.onProgress,
      onStart: opts.onStart,
      registry: dist.registry,
      unpackTo,
    })
  }
}

async function fetchFromLocalTarball (
  tarball: string,
  dir: string,
  opts: {
    ignore?: IgnoreFunction,
    integrity?: string,
  },
): Promise<FetchResult> {
  const tarballStream = fs.createReadStream(tarball)
  const tempLocation = pathTemp(dir)
  try {
    const filesIndex = (
      await Promise.all([
        unpackStream.local(
          tarballStream,
          tempLocation,
          {
            ignore: opts.ignore,
          },
        ),
        opts.integrity && (ssri.checkStream(tarballStream, opts.integrity) as any), // tslint:disable-line
      ])
    )[0]
    return { filesIndex, tempLocation }
  } catch (err) {
    rimraf(tempLocation, () => {
      // ignore errors
    })
    err.attempts = 1
    err.resource = tarball
    throw err
  }
}
