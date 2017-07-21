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
  }
): Got => {
  opts.rawNpmConfig['registry'] = normalizeRegistryUrl(opts.rawNpmConfig['registry'] || opts.registry)

  const getCredentialsByURI = npmGetCredentialsByURI.bind({
    get (key: string) {
      return opts.rawNpmConfig[key]
    }
  })

  const requestsQueue = new PQueue({
    concurrency: opts.networkConcurrency,
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
    onStart?: () => void,
    onProgress?: (downloaded: number, totalSize: number) => void,
    integrity?: string,
    generatePackageIntegrity: boolean,
  }): Promise<{}> {
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

      return new Promise((resolve, reject) => {
        client.fetch(url, {auth: shouldAuth && auth}, async (err: Error, res: IncomingMessage) => {
          if (err) return reject(err)

          if (res.statusCode !== 200) {
            return reject(new Error(`Invalid response: ${res.statusCode}`))
          }

          if (opts.onStart) opts.onStart()
          if (opts.onProgress && res.headers['content-length']) {
            const onProgress = opts.onProgress
            let downloaded = 0
            let size = +res.headers['content-length']
            res.on('data', (chunk: Buffer) => {
              downloaded += chunk.length
              onProgress(downloaded, size)
            })
          }

          const writeStream = createWriteStreamAtomic(saveto)

          const stream = res
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)

          Promise.all([
            opts.integrity && ssri.checkStream(res, opts.integrity),
            unpackStream.local(res, opts.unpackTo, {
              generateIntegrity: opts.generatePackageIntegrity,
            })
          ])
          .then(vals => resolve(vals[1]))
          .catch(reject)
        })
      })
    }, {priority: 1000}) // tarballs are requested first because they are bigger than metadata
  }

  return {
    getJSON: <any>R.memoize(getJSON), // tslint:disable-line
    download,
  }
}
