import logger from '@pnpm/logger'
import createWriteStreamAtomic = require('fs-write-stream-atomic')
import {IncomingMessage} from 'http'
import mkdirp = require('mkdirp-promise')
import RegClient = require('npm-registry-client')
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
    proxy?: {
      http?: string,
      https?: string,
      localAddress?: string,
    },
    ssl?: {
      certificate?: string,
      key?: string,
      ca?: string,
      strict?: boolean,
    },
    retry?: {
      count?: number,
      factor?: number,
      minTimeout?: number,
      maxTimeout?: number,
      randomize?: boolean,
    },
    userAgent?: string,
  },
): DownloadFunction => {
  const registryLog = logger('registry')
  const client = new RegClient({
    ...gotOpts,
    log: {
      ...registryLog,
      http: registryLog.debug.bind(null, 'http'),
      verbose: registryLog.debug.bind(null, 'http'),
    },
  })

  const retryOpts = gotOpts.retry

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

    function fetch (currentAttempt: number) {
      return new Promise((resolve, reject) => {
        client.fetch(url, {auth: shouldAuth && opts.auth || undefined}, async (err: Error, res: IncomingMessage) => {
          if (err) return reject(err)

          if (res.statusCode !== 200) {
            return reject(new Error(`Invalid response: ${res.statusCode}`))
          }

          // Is saved to a variable only because TypeScript 5.3 errors otherwise
          const contentLength = res.headers['content-length']
          const size = typeof contentLength === 'string'
            ? parseInt(contentLength, 10)
            : null
          if (opts.onStart) {
            opts.onStart(size, currentAttempt)
          }
          const onProgress = opts.onProgress
          let downloaded = 0
          res.on('data', (chunk: Buffer) => {
            downloaded += chunk.length
            if (onProgress) onProgress(downloaded)
          })

          const writeStream = createWriteStreamAtomic(saveto)

          const stream = res
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)

          Promise.all([
            opts.integrity && ssri.checkStream(res, opts.integrity),
            unpackStream.local(res, opts.unpackTo, {
              generateIntegrity: opts.generatePackageIntegrity,
              ignore: opts.ignore,
            }),
            waitTillClosed({ stream, size, getDownloaded: () => downloaded, url }),
          ])
          .then((vals) => resolve(vals[1]))
          .catch(reject)
        })
      })
      .catch((err) => {
        err.attempts = currentAttempt
        err.resource = url
        throw err
      })
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
