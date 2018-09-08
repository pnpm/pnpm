import { storeLogger } from '@pnpm/logger'
import {
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import getCredentialsByURI = require('credentials-by-uri')
import mem = require('mem')
import fs = require('mz/fs')
import path = require('path')
import * as unpackStream from 'unpack-stream'
import createDownloader, { DownloadFunction } from './createDownloader'
import pathTemp = require('path-temp')
import { PnpmError } from './errorTypes'
import ssri = require('ssri')

export type IgnoreFunction = (filename: string) => boolean

export default function (
  opts: {
    registry: string,
    rawNpmConfig: object,
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
  },
): { tarball: FetchFunction } {
  const download = createDownloader({
    alwaysAuth: opts.alwaysAuth || false,
    registry: opts.registry,
    ca: opts.ca,
    cert: opts.cert,
    key: opts.key,
    localAddress: opts.localAddress,
    proxy: opts.httpsProxy || opts.proxy,
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
  return {
    tarball: fetchFromTarball.bind(null, {
      fetchFromRemoteTarball: fetchFromRemoteTarball.bind(null, {
        download,
        ignoreFile: opts.ignoreFile,
        offline: opts.offline,
        getCredentialsByURI: mem((registry: string) => getCredentialsByURI(registry, opts.rawNpmConfig)),
      }),
      ignore: opts.ignoreFile,
    }),
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
      opts: FetchOptions
    ) => unpackStream.Index,
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
    const tarball = path.join(opts.prefix, resolution.tarball.slice(5))
    return fetchFromLocalTarball(tarball, target, {
      ignore: ctx.ignore,
      integrity: resolution.integrity,
    })
  }
  return ctx.fetchFromRemoteTarball(target, resolution, opts)
}

async function fetchFromRemoteTarball (
  ctx: {
    offline: boolean,
    download: DownloadFunction,
    ignoreFile: IgnoreFunction,
    getCredentialsByURI: (registry: string) => {
      scope: string,
      token: string | undefined,
      password: string | undefined,
      username: string | undefined,
      email: string | undefined,
      auth: string | undefined,
      alwaysAuth: string | undefined,
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
        storeLogger.warn(`Redownloading corrupted cached tarball: ${opts.cachedTarballLocation}`);
        break
      case 'ENOENT':
        break
      default:
        throw err
    }

    if (ctx.offline) {
      throw new PnpmError('NO_OFFLINE_TARBALL', `Could not find ${opts.cachedTarballLocation} in local registry mirror`)
    }
    const auth = dist.registry ? ctx.getCredentialsByURI(dist.registry) : undefined
    return await ctx.download(dist.tarball, opts.cachedTarballLocation, {
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
  }
): Promise<FetchResult> {
  const tempLocation = pathTemp(dir)
  const tarballStream = fs.createReadStream(tarball)
  try {
    const filesIndex = (await Promise.all([
      opts.integrity && ssri.checkStream(tarballStream, opts.integrity),
      unpackStream.local(
        tarballStream,
        tempLocation,
        {
          ignore: opts.ignore,
        },
      ),
    ]))[0]
    return {filesIndex, tempLocation}
  } catch (err) {
    err.attempts = 1
    err.resource = tarball
    throw err
  }
}
