import logger from '@pnpm/logger'
import {IncomingMessage} from 'http'
import fs = require('mz/fs')
import path = require('path')
import * as unpackStream from 'unpack-stream'
import createDownloader, {DownloadFunction} from './createDownloader'
import {PnpmError} from './errorTypes'

const fetchLogger = logger('fetch')

export type IgnoreFunction = (filename: string) => boolean

export interface FetchOptions {
  auth: object,
  cachedTarballLocation: string,
  pkgId: string,
  prefix: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
}

export default function (
  opts: {
    alwaysAuth: boolean,
    registry: string,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl: boolean,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
    userAgent?: string,
    ignoreFile?: IgnoreFunction,
    offline?: boolean,
  },
) {
  const download = createDownloader({
    alwaysAuth: opts.alwaysAuth,
    registry: opts.registry,
    proxy: {
      http: opts.proxy,
      https: opts.httpsProxy,
      localAddress: opts.localAddress
    },
    ssl: {
      certificate: opts.cert,
      key: opts.key,
      ca: opts.ca,
      strict: opts.strictSsl
    },
    retry: {
      count: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
      minTimeout: opts.fetchRetryMintimeout,
      maxTimeout: opts.fetchRetryMaxtimeout
    },
    userAgent: opts.userAgent,
  })
  return {
    tarball: fetchFromTarball.bind(null, {
      fetchFromRemoteTarball: fetchFromRemoteTarball.bind(null, {
        download,
        ignoreFile: opts.ignoreFile,
        offline: opts.offline,
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
    return fetchFromLocalTarball(target, path.join(opts.prefix, resolution.tarball.slice(5)), ctx.ignore)
  }
  return ctx.fetchFromRemoteTarball(target, resolution, opts)
}

async function fetchFromRemoteTarball (
  ctx: {
    offline: boolean,
    download: DownloadFunction,
    ignoreFile: IgnoreFunction,
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
    const index = await fetchFromLocalTarball(unpackTo, opts.cachedTarballLocation)
    fetchLogger.debug(`finish ${dist.integrity} ${dist.tarball}`)
    return index
  } catch (err) {
    if (err.code !== 'ENOENT') throw err

    if (ctx.offline) {
      throw new PnpmError('NO_OFFLINE_TARBALL', `Could not find ${opts.cachedTarballLocation} in local registry mirror`)
    }
    return await ctx.download(dist.tarball, opts.cachedTarballLocation, {
      auth: opts.auth as any,
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
  dir: string,
  tarball: string,
  ignore?: IgnoreFunction,
): Promise<unpackStream.Index> {
  return await unpackStream.local(
    fs.createReadStream(tarball),
    dir,
    {
      ignore,
    },
  ) as unpackStream.Index
}
