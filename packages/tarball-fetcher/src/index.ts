import PnpmError from '@pnpm/error'
import {
  Cafs,
  DeferredManifestPromise,
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
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
  }
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
      download,
      getCredentialsByURI: mem((registry: string) => getCreds(registry)),
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
