import createFetcher from 'fetch-from-npm-registry'
import createWriteStreamAtomic = require('fs-write-stream-atomic')
import {IncomingMessage} from 'http'
import mkdirp = require('mkdirp-promise')
import path = require('path')
import retry = require('retry')
import ssri = require('ssri')
import unpackStream = require('unpack-stream')
import urlLib = require('url')
import {BadTarballError} from './errorTypes'

export interface HttpResponse {
  body: string
}

export type DownloadFunction = (url: string, saveto: string, opts: {
  auth?: {
    scope: string,
    token: string | undefined,
    password: string | undefined,
    username: string | undefined,
    email: string | undefined,
    auth: string | undefined,
    alwaysAuth: string | undefined,
  },
  unpackTo: string,
  registry?: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
  ignore?: (filename: string) => boolean,
  integrity?: string
  generatePackageIntegrity?: boolean,
}) => Promise<{}>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void,
  fetch: (url: string, opts: {auth?: object}, cb: (err: Error, res: IncomingMessage) => void) => void,
}

export default (
  gotOpts: {
    alwaysAuth: boolean,
    registry: string,
    // proxy
    proxy?: string,
    localAddress?: string,
    // ssl
    ca?: string,
    cert?: string,
    key?: string,
    strictSSL?: boolean,
    // retry
    retry?: {
      retries?: number,
      factor?: number,
      minTimeout?: number,
      maxTimeout?: number,
      randomize?: boolean,
    },
    userAgent?: string,
  },
): DownloadFunction => {
  const fetchFromNpmRegistry = createFetcher(gotOpts)

  const retryOpts = {
    retries: 2,
    factor: 10,
    minTimeout: 1e4, // 10 seconds
    maxTimeout: 6e4, // 1 minute
    ...gotOpts.retry
  }

  return async function download (url: string, saveto: string, opts: {
    auth?: {
      scope: string,
      token: string | undefined,
      password: string | undefined,
      username: string | undefined,
      email: string | undefined,
      auth: string | undefined,
      alwaysAuth: string | undefined,
    },
    unpackTo: string,
    registry?: string,
    onStart?: (totalSize: number | null, attempt: number) => void,
    onProgress?: (downloaded: number) => void,
    ignore?: (filename: string) => boolean,
    integrity?: string,
    generatePackageIntegrity?: boolean,
  }): Promise<{}> {
    await mkdirp(path.dirname(saveto))

    // If a tarball is hosted on a different place than the manifest, only send
    // credentials on `alwaysAuth`
    const shouldAuth = opts.auth && (
      opts.auth.alwaysAuth ||
      !opts.registry ||
      urlLib.parse(url).host === urlLib.parse(opts.registry).host
    )

    const op = retry.operation(retryOpts)

    return new Promise((resolve, reject) => {
      op.attempt((currentAttempt) => {
        fetch(currentAttempt)
          .then(resolve)
          .catch((err) => {
            if (op.retry(err)) {
              return
            }
            reject(op.mainError())
          })
      })
    })

    async function fetch (currentAttempt: number) {
      try {
        const res = await fetchFromNpmRegistry(url, {auth: shouldAuth && opts.auth as any || undefined}) // tslint:disable-line

        if (res.status !== 200) {
          // TODO: throw a meaningfull error
          throw new Error(`Invalid response: ${res.status}`)
        }

        const contentLength = res.headers.has('content-length') && res.headers.get('content-length')
        const size = typeof contentLength === 'string'
          ? parseInt(contentLength, 10)
          : null
        if (opts.onStart) {
          opts.onStart(size, currentAttempt)
        }
        const onProgress = opts.onProgress
        let downloaded = 0
        res.body.on('data', (chunk: Buffer) => {
          downloaded += chunk.length
          if (onProgress) onProgress(downloaded)
        })

        const writeStream = createWriteStreamAtomic(saveto)

        return await new Promise((resolve, reject) => {
          const stream = res.body
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)

            Promise.all([
              opts.integrity && ssri.checkStream(res.body, opts.integrity),
              unpackStream.local(res.body, opts.unpackTo, {
                generateIntegrity: opts.generatePackageIntegrity,
                ignore: opts.ignore,
              }),
              waitTillClosed({ stream, size, getDownloaded: () => downloaded, url }),
            ])
            .then((vals) => resolve(vals[1]))
            .catch(reject)
        })
      } catch (err) {
        err.attempts = currentAttempt
        err.resource = url
        throw err
      }
    }
  }
}

function waitTillClosed (
  opts: {
    stream: NodeJS.ReadableStream,
    size: null | number,
    getDownloaded: () => number,
    url: string,
  },
) {
  return new Promise((resolve, reject) => {
    opts.stream.on('close', () => {
      const downloaded = opts.getDownloaded()
      if (opts.size !== null && opts.size !== downloaded) {
        const err = new BadTarballError({
          expectedSize: opts.size,
          receivedSize: downloaded,
          tarballUrl: opts.url,
        })
        reject(err)
        return
      }
      resolve()
    })
  })
}
