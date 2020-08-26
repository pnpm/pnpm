import { IncomingMessage, Server, ServerResponse } from 'http'
import { globalInfo } from '@pnpm/logger'
import {
  RequestPackageOptions,
  Resolution,
  StoreController,
  WantedDependency,
} from '@pnpm/store-controller-types'
import locking from './lock'
import http = require('http')

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

export default function (
  store: StoreController,
  opts: {
    path?: string
    port?: number
    hostname?: string
    ignoreStopRequests?: boolean
    ignoreUploadRequests?: boolean
  }
) {
  const rawManifestPromises = {}
  const filesPromises = {}

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
      let body: any = '' // eslint-disable-line
      req.on('data', (data) => {
        body += data // eslint-disable-line
      })
      req.on('end', async () => {
        try {
          if (body.length > 0) {
            body = JSON.parse(body)
          } else {
            body = {}
          }
          resolve(body)
        } catch (e) {
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
            if (pkgResponse['bundledManifest']) { // eslint-disable-line
              rawManifestPromises[body.msgId] = pkgResponse['bundledManifest'] // eslint-disable-line
              pkgResponse.body['fetchingBundledManifestInProgress'] = true // eslint-disable-line
          }
            if (pkgResponse['files']) { // eslint-disable-line
              filesPromises[body.msgId] = pkgResponse['files'] // eslint-disable-line
          }
          res.end(JSON.stringify(pkgResponse.body))
        } catch (err) {
          res.end(JSON.stringify({
            error: {
              message: err.message,
              ...JSON.parse(JSON.stringify(err)),
            },
          }))
        }
        break
      }
      case '/fetchPackage': {
        try {
          body = await bodyPromise
          const pkgResponse = store.fetchPackage(body.options as RequestPackageOptions & {force: boolean, pkgId: string, resolution: Resolution})
            if (pkgResponse['bundledManifest']) { // eslint-disable-line
              rawManifestPromises[body.msgId] = pkgResponse['bundledManifest'] // eslint-disable-line
          }
            if (pkgResponse['files']) { // eslint-disable-line
              filesPromises[body.msgId] = pkgResponse['files'] // eslint-disable-line
          }
          res.end(JSON.stringify({ filesIndexFile: pkgResponse.filesIndexFile }))
        } catch (err) {
          res.end(JSON.stringify({
            error: {
              message: err.message,
              ...JSON.parse(JSON.stringify(err)),
            },
          }))
        }
        break
      }
      case '/packageFilesResponse': {
        body = await bodyPromise
        const filesResponse = await filesPromises[body.msgId]()
        delete filesPromises[body.msgId]
        res.end(JSON.stringify(filesResponse))
        break
      }
      case '/rawManifestResponse': {
        body = await bodyPromise
        const manifestResponse = await rawManifestPromises[body.msgId]()
        delete rawManifestPromises[body.msgId]
        res.end(JSON.stringify(manifestResponse))
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
        res.end(JSON.stringify('OK'))
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
        await lock(uploadBody.builtPkgLocation, () => store.upload(uploadBody.builtPkgLocation, uploadBody.opts))
        res.end(JSON.stringify('OK'))
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
        res.end(JSON.stringify('OK'))
        globalInfo('Server stopped')
        break
      default: {
        res.statusCode = 404
        const error = { error: `${req.url!} does not match any route` }
        res.end(JSON.stringify(error))
      }
      }
    } catch (e) {
      res.statusCode = 503
      const jsonErr = JSON.parse(JSON.stringify(e))
      jsonErr.message = e.message
      res.end(JSON.stringify(jsonErr))
    }
  })

  let listener: Server
  if (opts.path) {
    listener = server.listen(opts.path)
  } else {
    listener = server.listen(opts.port, opts.hostname)
  }

  return { close }

  function close () {
    listener.close()
    return store.close()
  }
}
