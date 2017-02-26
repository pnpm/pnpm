import {IncomingMessage} from 'http'
import getRegistryAuthInfo = require('registry-auth-token')
import memoize = require('lodash.memoize')
import pLimit = require('p-limit')
import crypto = require('crypto')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import createWriteStreamAtomic = require('fs-write-stream-atomic')

export type RequestParams = {
  auth?: {
    token: string
  } | {
    username: string,
    password: string
  }
}

export type HttpResponse = {
  body: string
}

export type Got = {
  download(url: string, saveto: string, opts: {
    onStart?: () => void,
    onProgress?: (downloaded: number, totalSize: number) => void,
    shasum?: string
  }): Promise<void>,
  getJSON<T>(url: string): Promise<T>,
}

export type NpmRegistryClient = {
  get: Function,
  fetch: Function
}

export default (client: NpmRegistryClient, opts: {networkConcurrency: number}): Got => {
  const limit = pLimit(opts.networkConcurrency)

  async function getJSON (url: string) {
    return limit(() => new Promise((resolve, reject) => {
      client.get(url, createOptions(url), (err: Error, data: Object, raw: Object, res: HttpResponse) => {
        if (err) return reject(err)
        resolve(data)
      })
    }))
  }

  function download (url: string, saveto: string, opts: {
    onStart?: () => void,
    onProgress?: (downloaded: number, totalSize: number) => void,
    shasum?: string
  }): Promise<void> {
    return limit(async () => {
      await mkdirp(path.dirname(saveto))

      return new Promise((resolve, reject) => {
        client.fetch(url, createOptions(url), async (err: Error, res: IncomingMessage) => {
          if (err) return reject(err)
          const writeStream = createWriteStreamAtomic(saveto)
          const actualShasum = crypto.createHash('sha1')

          res
            .on('response', start)
            .on('data', (_: Buffer) => { actualShasum.update(_) })
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)
            .on('finish', finish)

          function start (res: IncomingMessage) {
            if (res.statusCode !== 200) {
              return reject(new Error(`Invalid response: ${res.statusCode}`))
            }

            if (opts.onStart) opts.onStart()
            if (opts.onProgress && ('content-length' in res.headers)) {
              const onProgress = opts.onProgress
              let downloaded = 0
              let size = +res.headers['content-length']
              res.on('data', (chunk: Buffer) => {
                downloaded += chunk.length
                onProgress(downloaded, size)
              })
            }
          }

          async function finish () {
            const digest = actualShasum.digest('hex')
            if (opts.shasum && digest !== opts.shasum) {
              reject(new Error(`Incorrect shasum (expected ${opts.shasum}, got ${digest})`))
              return
            }

            resolve()
          }
        })
      })
    })
  }

  function createOptions (url: string): RequestParams {
    const authInfo = getRegistryAuthInfo(url, {recursive: true})
    if (!authInfo) return {}
    switch (authInfo.type) {
      case 'Bearer':
        return {
          auth: {
            token: authInfo.token
          }
        }
      case 'Basic':
        return {
          auth: {
            username: authInfo.username,
            password: authInfo.password
          }
        }
      default:
        throw new Error(`Unsupported authorization type '${authInfo.type}'`)
    }
  }

  return {
    getJSON: memoize(getJSON),
    download,
  }
}
