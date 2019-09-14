import { storeLogger } from '@pnpm/logger'
import {
  RequestPackageOptions,
  Resolution,
  StoreController,
  WantedDependency,
} from '@pnpm/store-controller-types'
import http = require('http')
import { IncomingMessage, Server, ServerResponse } from 'http'

import locking from './lock'

interface RequestBody {
  msgId: string,
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
  prefix: string,
  opts: {
    addDependencies: string[];
    removeDependencies: string[];
    prune: boolean;
  },
  storePath: string,
  id: string,
  searchQueries: string[],
}

export default function (
  store: StoreController,
  opts: {
    path?: string,
    port?: number,
    hostname?: string,
    ignoreStopRequests?: boolean,
    ignoreUploadRequests?: boolean,
  },
) {
  const rawManifestPromises = {}
  const filesPromises = {}

  const lock = locking<void>()

  const server = http.createServer(async (req: IncomingMessage, res: ServerResponse) => {
    if (req.method !== 'POST') {
      res.statusCode = 405  // Method Not Allowed
      const responseError = { error: `Only POST is allowed, received ${req.method}` }
      res.setHeader('Allow', 'POST')
      res.end(JSON.stringify(responseError))
      return
    }

    const bodyPromise = new Promise<RequestBody>((resolve, reject) => {
      let body: any = '' // tslint:disable-line
      req.on('data', (data) => {
        body += data
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
            if (pkgResponse['bundledManifest']) { // tslint:disable-line
              rawManifestPromises[body.msgId] = pkgResponse['bundledManifest'] // tslint:disable-line
              pkgResponse.body['fetchingBundledManifestInProgress'] = true // tslint:disable-line
            }
            if (pkgResponse['files']) { // tslint:disable-line
              filesPromises[body.msgId] = pkgResponse['files'] // tslint:disable-line
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
            if (pkgResponse['bundledManifest']) { // tslint:disable-line
              rawManifestPromises[body.msgId] = pkgResponse['bundledManifest'] // tslint:disable-line
            }
            if (pkgResponse['files']) { // tslint:disable-line
              filesPromises[body.msgId] = pkgResponse['files'] // tslint:disable-line
            }
            res.end(JSON.stringify({ inStoreLocation: pkgResponse.inStoreLocation }))
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
        case '/packageFilesResponse':
          body = await bodyPromise
          const filesResponse = await filesPromises[body.msgId]()
          delete filesPromises[body.msgId]
          res.end(JSON.stringify(filesResponse))
          break
        case '/rawManifestResponse':
          body = await bodyPromise
          const manifestResponse = await rawManifestPromises[body.msgId]()
          delete rawManifestPromises[body.msgId]
          res.end(JSON.stringify(manifestResponse))
          break
        case '/updateConnections':
          body = await bodyPromise
          await store.updateConnections(body.prefix, body.opts)
          res.end(JSON.stringify('OK'))
          break
        case '/prune':
          // Disable store pruning when a server is running
          res.statusCode = 403
          res.end()
          break
        case '/saveState':
          await store.saveState()
          res.end(JSON.stringify('OK'))
          break
        case '/importPackage':
          const importPackageBody = (await bodyPromise) as any // tslint:disable-line:no-any
          await store.importPackage(importPackageBody.from, importPackageBody.to, importPackageBody.opts)
          res.end(JSON.stringify('OK'))
          break
        case '/upload':
          // Do not return an error status code, just ignore the upload request entirely
          if (opts.ignoreUploadRequests) {
            res.statusCode = 403
            res.end()
            break
          }
          const uploadBody = (await bodyPromise) as any // tslint:disable-line:no-any
          await lock(uploadBody.builtPkgLocation, () => store.upload(uploadBody.builtPkgLocation, uploadBody.opts))
          res.end(JSON.stringify('OK'))
          break
        case '/stop':
          if (opts.ignoreStopRequests) {
            res.statusCode = 403
            res.end()
            break
          }
          storeLogger.info('Got request to stop the server')
          await close()
          res.end(JSON.stringify('OK'))
          storeLogger.info('Server stopped')
          break
        case '/getPackageLocation': {
          const { packageId, packageName, opts } = (await bodyPromise) as any // tslint:disable-line:no-any
          const pkgLocation = await store.getPackageLocation(packageId, packageName, opts)
          res.end(JSON.stringify(pkgLocation))
          break
        }
        case '/findPackageUsages':
          body = await bodyPromise
          res.end(JSON.stringify(await store.findPackageUsages(body.searchQueries)))
          break
        default:
          res.statusCode = 404
          const error = { error: `${req.url} does not match any route` }
          res.end(JSON.stringify(error))
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
