import http = require('http')
import {IncomingMessage, Server, ServerResponse} from 'http'

import {RequestPackageOptions, WantedDependency} from '@pnpm/package-requester'
import {StoreController} from 'package-store'

interface RequestBody {
  pkgId: string,
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
  prefix: string,
  opts: {
    addDependencies: string[];
    removeDependencies: string[];
    prune: boolean;
  }
}

export default function (
  store: StoreController,
  opts: {
    path?: string,
    port?: number,
    hostname?: string,
  },
) {
  const manifestPromises = {}
  const filesPromises = {}

  const server = http.createServer(async (req: IncomingMessage, res: ServerResponse) => {
    if (req.method !== 'POST') {
      res.statusCode = 503
      res.end(JSON.stringify(`Only POST is allowed, received ${req.method}`))
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
        case '/requestPackage':
          body = await bodyPromise
          const pkgResponse = await store.requestPackage(body.wantedDependency, body.options)
          if (pkgResponse['fetchingManifest']) { // tslint:disable-line
            manifestPromises[pkgResponse.body.id] = pkgResponse['fetchingManifest'] // tslint:disable-line
          }
          if (pkgResponse['fetchingFiles']) { // tslint:disable-line
            filesPromises[pkgResponse.body.id] = pkgResponse['fetchingFiles'] // tslint:disable-line
          }
          res.end(JSON.stringify(pkgResponse.body))
          break
        case '/packageFilesResponse':
          body = await bodyPromise
          const filesResponse = await filesPromises[body.pkgId]
          delete filesPromises[body.pkgId]
          res.end(JSON.stringify(filesResponse))
          break
        case '/manifestResponse':
          body = await bodyPromise
          const manifestResponse = await manifestPromises[body.pkgId]
          delete manifestPromises[body.pkgId]
          res.end(JSON.stringify(manifestResponse))
          break
        case '/updateConnections':
          body = await bodyPromise
          await store.updateConnections(body.prefix, body.opts)
          res.end(JSON.stringify('OK'))
          break
        case '/prune':
          await store.prune()
          res.end(JSON.stringify('OK'))
          break
        case '/saveState':
          await store.saveState()
          res.end(JSON.stringify('OK'))
          break
        default:
          res.statusCode = 404
          res.end(`${req.url} does not match any route`)
      }
    } catch (e) {
      res.statusCode = 503
      res.end(JSON.stringify(e.message))
    }
  })

  let listener: Server;
  if (opts.path) {
    listener = server.listen(opts.path)
  } else {
    listener = server.listen(opts.port, opts.hostname)
  }

  return {
    close: () => listener.close(() => { return }),
  }
}
