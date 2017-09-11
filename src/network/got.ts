import {IncomingMessage} from 'http'
import R = require('ramda')
import pLimit = require('p-limit')
import crypto = require('crypto')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import createWriteStreamAtomic = require('fs-write-stream-atomic')
import ssri = require('ssri')
import unpackStream = require('unpack-stream')
import npmGetCredentialsByURI = require('npm/lib/config/get-credentials-by-uri')
import urlLib = require('url')
import normalizeRegistryUrl = require('normalize-registry-url')
import PQueue = require('p-queue')
import {progressLogger} from 'pnpm-logger'
import retry = require('retry')

export type AuthInfo = {
  alwaysAuth: boolean,
} & ({
  token: string,
} | {
  username: string,
  password: string,
})

export type HttpResponse = {
  body: string
}

export type Got = {
  download(url: string, saveto: string, opts: {
    unpackTo: string,
    registry?: string,
    onStart?: (totalSize: number | null, attempt: number) => void,
    onProgress?: (downloaded: number) => void,
    integrity?: string
    generatePackageIntegrity?: boolean,
  }): Promise<{}>,
  getJSON<T>(url: string, registry: string, priority?: number): Promise<T>,
}

export type NpmRegistryClient = {
  get: Function,
  fetch: Function
}

export default (
  client: NpmRegistryClient,
  opts: {
    networkConcurrency: number,
    rawNpmConfig: Object,
    alwaysAuth: boolean,
    registry: string,
    retries?: number,
    factor?: number,
    minTimeout?: number,
    maxTimeout?: number,
    randomize?: boolean,
  }
): Got => {
  opts.rawNpmConfig['registry'] = normalizeRegistryUrl(opts.rawNpmConfig['registry'] || opts.registry)
  const retryOpts = {
    retries: opts.retries,
    factor: opts.factor,
    minTimeout: opts.minTimeout,
    maxTimeout: opts.maxTimeout,
    randomize: opts.randomize,
  }

  const getCredentialsByURI = npmGetCredentialsByURI.bind({
    get (key: string) {
      return opts.rawNpmConfig[key]
    }
  })

  let counter = 0
  const networkConcurrency = opts.networkConcurrency || 16
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })

  async function getJSON (url: string, registry: string, priority?: number) {
    return requestsQueue.add(() => new Promise((resolve, reject) => {
    const getOpts = {
        auth: getCredentialsByURI(registry),
        fullMetadata: false,
      }
      client.get(url, getOpts, (err: Error, data: Object, raw: Object, res: HttpResponse) => {
        if (err) return reject(err)
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

      const auth = opts.registry && getCredentialsByURI(opts.registry)
      // If a tarball is hosted on a different place than the manifest, only send
      // credentials on `alwaysAuth`
      const shouldAuth = auth && (
        auth.alwaysAuth ||
        !opts.registry ||
        urlLib.parse(url).host === urlLib.parse(opts.registry).host
      )

      const op = retry.operation(retryOpts)

      return new Promise((resolve, reject) => {
        op.attempt(currentAttempt => {
          fetch(currentAttempt)
            .then(resolve)
            .catch(err => {
              if (op.retry(err)) {
                return
              }
              reject(op.mainError())
            })
        })

        function fetch (currentAttempt: number) {
          return new Promise((resolve, reject) => {
            client.fetch(url, {auth: shouldAuth && auth}, async (err: Error, res: IncomingMessage) => {
              if (err) return reject(err)

              if (res.statusCode !== 200) {
                return reject(new Error(`Invalid response: ${res.statusCode}`))
              }

              const size = res.headers['content-length']
                ? parseInt(res.headers['content-length'])
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
                new Promise((resolve, reject) => {
                  stream.on('close', () => {
                    if (size !== null && size !== downloaded) {
                      const err = new Error(`Actual size (${downloaded}) of tarball (${url}) did not match the one specified in \'Content-Length\' header (${size})`)
                      err['code'] = 'BAD_TARBALL_SIZE'
                      err['expectedSize'] = size
                      err['receivedSize'] = downloaded
                      reject(err)
                      return
                    }
                    resolve()
                  })
                }),
              ])
              .then(vals => resolve(vals[1]))
              .catch(reject)
            })
          })
          .catch(err => {
            err['attempts'] = currentAttempt
            err['resource'] = url
            throw err
          })
        }
      })
    }, {priority})
  }

  return {
    getJSON: <any>R.memoize(getJSON), // tslint:disable-line
    download,
  }
}
