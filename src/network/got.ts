import {IncomingMessage} from 'http'
import getRegistryAuthInfo = require('registry-auth-token')
import R = require('ramda')
import pLimit = require('p-limit')
import crypto = require('crypto')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import createWriteStreamAtomic = require('fs-write-stream-atomic')
import ssri = require('ssri')
import unpackStream = require('unpack-stream')

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
    onStart?: () => void,
    onProgress?: (downloaded: number, totalSize: number) => void,
    integrity?: string
  }): Promise<{}>,
  getJSON<T>(url: string): Promise<T>,
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
  }
): Got => {
  const limit = pLimit(opts.networkConcurrency)

  async function getJSON (url: string) {
    return limit(() => new Promise((resolve, reject) => {
      const getOpts = {
        auth: getAuth(url),
        fullMetadata: false,
      }
      client.get(url, getOpts, (err: Error, data: Object, raw: Object, res: HttpResponse) => {
        if (err) return reject(err)
        resolve(data)
      })
    }))
  }

  function download (url: string, saveto: string, opts: {
    unpackTo: string,
    registry?: string,
    onStart?: () => void,
    onProgress?: (downloaded: number, totalSize: number) => void,
    integrity?: string
  }): Promise<{}> {
    return limit(async () => {
      await mkdirp(path.dirname(saveto))

      const auth = getAuth(url) || opts.registry && getAuth(opts.registry)

      return new Promise((resolve, reject) => {
        client.fetch(url, {auth}, async (err: Error, res: IncomingMessage) => {
          if (err) return reject(err)
          const writeStream = createWriteStreamAtomic(saveto)

          const stream = res
            .on('response', start)
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)

          Promise.all([
            opts.integrity && ssri.checkStream(res, opts.integrity),
            unpackStream.local(res, opts.unpackTo)
          ])
          .then(vals => resolve(vals[1]))
          .catch(reject)

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
        })
      })
    })
  }

  function getAuth (url: string): AuthInfo | null {
    const authInfo = getRegistryAuthInfo(url, {recursive: true, npmrc: opts.rawNpmConfig})

    if (!authInfo) return null
    switch (authInfo.type) {
      case 'Bearer':
        return {
          alwaysAuth: opts.alwaysAuth,
          token: authInfo.token,
        }
      case 'Basic':
        return {
          alwaysAuth: opts.alwaysAuth,
          username: authInfo.username,
          password: authInfo.password,
        }
      default:
        throw new Error(`Unsupported authorization type '${authInfo.type}'`)
    }
  }

  return {
    getJSON: <any>R.memoize(getJSON), // tslint:disable-line
    download,
  }
}
