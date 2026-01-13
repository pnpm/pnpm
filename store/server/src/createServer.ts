import assert from 'assert'
import http, { type IncomingMessage, type Server, type ServerResponse } from 'http'
import util from 'util'

import { globalInfo } from '@pnpm/logger'
import {
  type PkgRequestFetchResult,
  type RequestPackageOptions,
  type StoreController,
  type WantedDependency,
  type FetchPackageToStoreFunction,
} from '@pnpm/store-controller-types'
import { locking } from './lock.js'

function replacer (key: unknown, value: unknown) {
  if (value instanceof Map || Object.prototype.toString.call(value) === '[object Map]') {
    return {
      dataType: 'Map',
      // @ts-expect-error
      value: Array.from(value.entries()),
    }
  }
  return value
}

function reviver (key: unknown, value: unknown) {
  if (typeof value === 'object' && value !== null) {
    // @ts-expect-error
    if (value.dataType === 'Map') {
      // @ts-expect-error
      return new Map(value.value)
    }
  }
  return value
}

interface RequestBody {
  msgId: string
  wantedDependency: WantedDependency
  options: RequestPackageOptions
  prefix: string
  opts: {
    addDependencies: string[]
    removeDependencies: string[]
    prune: boolean
  }
  storePath: string
  id: string
  searchQueries: string[]
}

export interface StoreServerHandle {
  close: () => Promise<void>
  waitForListen: Promise<void>
  waitForClose: Promise<void>
}

export function createServer (
  store: StoreController,
  opts: {
    path?: string
    port?: number
    hostname?: string
    ignoreStopRequests?: boolean
    ignoreUploadRequests?: boolean
  }
): StoreServerHandle {
  const filesPromises: Record<string, () => Promise<PkgRequestFetchResult>> = {}

  // eslint-disable-next-line @typescript-eslint/no-invalid-void-type
  const lock = locking<void>()

  const server = http.createServer(async (req: IncomingMessage, res: ServerResponse) => {
    if (req.method !== 'POST') {
      res.statusCode = 405 // Method Not Allowed
      const responseError = { error: `Only POST is allowed, received ${req.method ?? 'unknown'}` }
      res.setHeader('Allow', 'POST')
      res.end(JSON.stringify(responseError))
      return
    }

    const bodyPromise = new Promise<RequestBody>((resolve, reject) => {
      const chunks: Buffer[] = []
      req.on('data', (chunk) => {
        chunks.push(chunk)
      })
      req.on('end', async () => {
        try {
          const bodyBuffer = Buffer.concat(chunks)
          let body: any // eslint-disable-line
          if (bodyBuffer.byteLength > 0) {
            body = JSON.parse(bodyBuffer.toString(), reviver)
          } else {
            body = {}
          }
          resolve(body)
        } catch (e: any) { // eslint-disable-line
          reject(e)
        }
      })
    })

    try {
      let body: RequestBody
      switch (req.url) {
      case '/requestPackage': {
        try {
          body = await bodyPromise
          const pkgResponse = await store.requestPackage(body.wantedDependency, body.options)
          if (pkgResponse.fetching) {
            filesPromises[body.msgId] = pkgResponse.fetching
          }
          res.end(JSON.stringify(pkgResponse.body, replacer))
        } catch (err: unknown) {
          assert(util.types.isNativeError(err))
          res.end(JSON.stringify({
            error: {
              message: err.message,
              ...JSON.parse(JSON.stringify(err)),
            },
          }, replacer))
        }
        break
      }
      case '/fetchPackage': {
        try {
          body = await bodyPromise
          const pkgResponse = (store.fetchPackage as FetchPackageToStoreFunction)(body.options as any) // eslint-disable-line
          filesPromises[body.msgId] = pkgResponse.fetching
          res.end(JSON.stringify({ filesIndexFile: pkgResponse.filesIndexFile }, replacer))
        } catch (err: unknown) {
          assert(util.types.isNativeError(err))
          res.end(JSON.stringify({
            error: {
              message: err.message,
              ...JSON.parse(JSON.stringify(err)),
            },
          }, replacer))
        }
        break
      }
      case '/packageFilesResponse': {
        body = await bodyPromise
        const filesResponse = await filesPromises[body.msgId]()
        delete filesPromises[body.msgId]
        res.end(JSON.stringify(filesResponse, replacer))
        break
      }
      case '/prune':
        // Disable store pruning when a server is running
        res.statusCode = 403
        res.end()
        break
      case '/importPackage': {
        const importPackageBody = (await bodyPromise) as any // eslint-disable-line @typescript-eslint/no-explicit-any
        await store.importPackage(importPackageBody.to, importPackageBody.opts)
        res.end(JSON.stringify('OK', replacer))
        break
      }
      case '/upload': {
        // Do not return an error status code, just ignore the upload request entirely
        if (opts.ignoreUploadRequests) {
          res.statusCode = 403
          res.end()
          break
        }
        const uploadBody = (await bodyPromise) as any // eslint-disable-line @typescript-eslint/no-explicit-any
        await lock(uploadBody.builtPkgLocation, async () => store.upload(uploadBody.builtPkgLocation, uploadBody.opts))
        res.end(JSON.stringify('OK', replacer))
        break
      }
      case '/stop':
        if (opts.ignoreStopRequests) {
          res.statusCode = 403
          res.end()
          break
        }
        globalInfo('Got request to stop the server')
        await close()
        res.end(JSON.stringify('OK', replacer))
        globalInfo('Server stopped')
        break
      default: {
        res.statusCode = 404
        const error = { error: `${req.url!} does not match any route` }
        res.end(JSON.stringify(error, replacer))
      }
      }
    } catch (e: any) { // eslint-disable-line
      res.statusCode = 503
      const jsonErr = JSON.parse(JSON.stringify(e))
      jsonErr.message = e.message
      res.end(JSON.stringify(jsonErr, replacer))
    }
  })

  let listener: Server

  const waitForListen = new Promise<void>((resolve) => {
    if (opts.path) {
      listener = server.listen(opts.path, () => {
        resolve()
      })
    } else {
      listener = server.listen(opts.port, opts.hostname, () => {
        resolve()
      })
    }
  })

  const waitForClose = new Promise<void>((resolve) => listener.once('close', () => {
    resolve()
  }))

  return { close, waitForListen, waitForClose }

  async function close (): Promise<void> {
    listener.close()
    return store.close()
  }
}
