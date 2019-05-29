import { FetchResult } from '@pnpm/fetcher-base'
import logger from '@pnpm/logger'
import createFetcher from 'fetch-from-npm-registry'
import fs = require('graceful-fs')
import { IncomingMessage } from 'http'
import makeDir = require('make-dir')
import path = require('path')
import pathTemp = require('path-temp')
import retry = require('retry')
import rimraf = require('rimraf')
import ssri = require('ssri')
import unpackStream = require('unpack-stream')
import urlLib = require('url')
import { BadTarballError } from './errorTypes'

const ignorePackageFileLogger = logger('_ignore-package-file')

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
}) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void,
  fetch: (url: string, opts: {auth?: object}, cb: (err: Error, res: IncomingMessage) => void) => void,
}

export default (
  gotOpts: {
    alwaysAuth: boolean,
    fsIsCaseSensitive: boolean,
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
    factor: 10,
    maxTimeout: 6e4, // 1 minute
    minTimeout: 1e4, // 10 seconds
    retries: 2,
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
  }): Promise<FetchResult> {
    const saveToDir = path.dirname(saveto)
    await makeDir(saveToDir)

    // If a tarball is hosted on a different place than the manifest, only send
    // credentials on `alwaysAuth`
    const shouldAuth = opts.auth && (
      opts.auth.alwaysAuth ||
      !opts.registry ||
      urlLib.parse(url).host === urlLib.parse(opts.registry).host
    )

    const op = retry.operation(retryOpts)

    return new Promise<FetchResult>((resolve, reject) => {
      op.attempt((currentAttempt) => {
        fetch(currentAttempt)
          .then(resolve)
          .catch((err) => {
            if (err.httpStatusCode === 403) {
              reject(err)
              return
            }
            if (op.retry(err)) {
              return
            }
            reject(op.mainError())
          })
      })
    })

    async function fetch (currentAttempt: number): Promise<FetchResult> {
      try {
        const res = await fetchFromNpmRegistry(url, {auth: shouldAuth && opts.auth as any || undefined}) // tslint:disable-line

        if (res.status !== 200) {
          const err = new Error(`${res.status} ${res.statusText}: ${url}`)
          // tslint:disable
          err['code'] = 'ERR_PNPM_TARBALL_FETCH'
          err['httpStatusCode'] = res.status
          err['uri'] = url
          err['response'] = res
          // tslint:enable
          throw err
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

        const tempTarballLocation = pathTemp(saveToDir)
        const writeStream = fs.createWriteStream(tempTarballLocation)

        return await new Promise<FetchResult>((resolve, reject) => {
          const stream = res.body
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)

          const tempLocation = pathTemp(opts.unpackTo)
          const ignore = gotOpts.fsIsCaseSensitive ? opts.ignore : createIgnorer(url, opts.ignore)
          Promise.all([
            opts.integrity && safeCheckStream(res.body, opts.integrity) || true,
            unpackStream.local(res.body, tempLocation, {
              generateIntegrity: opts.generatePackageIntegrity,
              ignore,
            }),
            waitTillClosed({ stream, size, getDownloaded: () => downloaded, url }),
          ])
          .then(([integrityCheckResult, filesIndex]) => {
            if (integrityCheckResult !== true) {
              throw integrityCheckResult
            }
            fs.rename(tempTarballLocation, saveto, () => {
              // ignore errors
            })
            resolve({ tempLocation, filesIndex })
          })
          .catch((err) => {
            rimraf(tempTarballLocation, () => {
              // ignore errors
            })
            rimraf(tempLocation, () => {
              // Just ignoring this error
              // A redundant stage folder won't break anything
            })
            reject(err)
          })
        })
      } catch (err) {
        err.attempts = currentAttempt
        err.resource = url
        throw err
      }
    }
  }
}

function createIgnorer (tarballUrl: string, ignore?: (filename: string) => Boolean) {
  const lowercaseFiles = new Set<string>()
  if (ignore) {
    return (filename: string) => {
      const lowercaseFilename = filename.toLowerCase()
      if (lowercaseFiles.has(lowercaseFilename)) {
        ignorePackageFileLogger.debug({
          reason: 'case-insensitive-duplicate',
          skippedFilename: filename,
          tarballUrl,
        })
        return true
      }
      lowercaseFiles.add(lowercaseFilename)
      return ignore(filename)
    }
  }
  return (filename: string) => {
    const lowercaseFilename = filename.toLowerCase()
    if (lowercaseFiles.has(lowercaseFilename)) {
      ignorePackageFileLogger.debug({
        reason: 'case-insensitive-duplicate',
        skippedFilename: filename,
        tarballUrl,
      })
      return true
    }
    lowercaseFiles.add(lowercaseFilename)
    return false
  }
}

async function safeCheckStream (stream: any, integrity: string): Promise<true | Error> { // tslint:disable-line:no-any
  try {
    await ssri.checkStream(stream, integrity)
    return true
  } catch (err) {
    return err
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
