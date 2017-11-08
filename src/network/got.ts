import getCredentialsByURI = require('credentials-by-uri')
import crypto = require('crypto')
import createWriteStreamAtomic = require('fs-write-stream-atomic')
import {IncomingMessage} from 'http'
import mkdirp = require('mkdirp-promise')
import normalizeRegistryUrl = require('normalize-registry-url')
import pLimit = require('p-limit')
import PQueue = require('p-queue')
import path = require('path')
import R = require('ramda')
import retry = require('retry')
import ssri = require('ssri')
import unpackStream = require('unpack-stream')
import urlLib = require('url')
import {BadTarballError} from '../errorTypes'
import {progressLogger} from '../loggers'

export type AuthInfo = {
  alwaysAuth: boolean,
} & ({
  token: string,
} | {
  username: string,
  password: string,
})

export interface HttpResponse {
  body: string
}

export interface Got {
  download (url: string, saveto: string, opts: {
    unpackTo: string,
    registry?: string,
    onStart?: (totalSize: number | null, attempt: number) => void,
    onProgress?: (downloaded: number) => void,
    integrity?: string
    generatePackageIntegrity?: boolean,
  }): Promise<{}>,
  getJSON<T> (url: string, registry: string, priority?: number): Promise<T>,
}

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void,
  fetch: (url: string, opts: {auth?: object}, cb: (err: Error, res: IncomingMessage) => void) => void,
}

export default (
  client: NpmRegistryClient,
  gotOpts: {
    networkConcurrency: number,
    rawNpmConfig: object & { registry?: string },
    alwaysAuth: boolean,
    registry: string,
    retries?: number,
    factor?: number,
    minTimeout?: number,
    maxTimeout?: number,
    randomize?: boolean,
  },
): Got => {
  gotOpts.rawNpmConfig.registry = normalizeRegistryUrl(gotOpts.rawNpmConfig.registry || gotOpts.registry)
  const retryOpts = {
    factor: gotOpts.factor,
    maxTimeout: gotOpts.maxTimeout,
    minTimeout: gotOpts.minTimeout,
    randomize: gotOpts.randomize,
    retries: gotOpts.retries,
  }

  let counter = 0
  const networkConcurrency = gotOpts.networkConcurrency || 16
  const rawNpmConfig = gotOpts.rawNpmConfig || {}
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })

  async function getJSON (url: string, registry: string, priority?: number) {
    return requestsQueue.add(() => new Promise((resolve, reject) => {
    const getOpts = {
        auth: getCredentialsByURI(registry, rawNpmConfig),
        fullMetadata: false,
      }
      client.get(url, getOpts, (err: Error, data: object, raw: object, res: HttpResponse) => {
        if (err) {
          reject(err)
          return
        }
        resolve(data)
      })
    }), { priority })
  }

  function download (url: string, saveto: string, opts: {
    unpackTo: string,
    registry?: string,
    onStart?: (totalSize: number | null, attempt: number) => void,
    onProgress?: (downloaded: number) => void,
    integrity?: string,
    generatePackageIntegrity?: boolean,
  }): Promise<{}> {
    // Tarballs are requested first because they are bigger than metadata files.
    // However, when one line is left available, allow it to be picked up by a metadata request.
    // This is done in order to avoid situations when tarballs are downloaded in chunks
    // As much tarballs should be downloaded simultaneously as possible.
    const priority = (++counter % networkConcurrency === 0 ? -1 : 1) * 1000

    return requestsQueue.add(async () => {
      await mkdirp(path.dirname(saveto))

      const auth = opts.registry && getCredentialsByURI(opts.registry, rawNpmConfig)
      // If a tarball is hosted on a different place than the manifest, only send
      // credentials on `alwaysAuth`
      const shouldAuth = auth && (
        auth.alwaysAuth ||
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
          client.fetch(url, {auth: shouldAuth && auth || undefined}, async (err: Error, res: IncomingMessage) => {
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
    }, {priority})
  }

  return {
    download,
    getJSON: <any>R.memoize(getJSON), // tslint:disable-line
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
