import http, {
  type Server,
  type ServerResponse,
  type IncomingMessage,
} from 'http'

import { globalInfo } from '@pnpm/logger'

import type {
  StoreController,
  WantedDependency,
  PkgRequestFetchResult,
  RequestPackageOptions,
  FetchPackageToStoreFunction,
} from '@pnpm/types'

import { locking } from './lock.js'

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

export function createServer(
  store: StoreController,
  opts: {
    path?: string | undefined
    port?: number | undefined
    hostname?: string | undefined
    ignoreStopRequests?: boolean | undefined
    ignoreUploadRequests?: boolean | undefined
  }
): {
    close: () => Promise<void>;
    waitForClose: Promise<void>;
  } {
  const filesPromises: Record<string, (() => Promise<PkgRequestFetchResult>) | undefined> | undefined = {}

  const lock = locking<void>()

  const server = http.createServer(
    async (req: IncomingMessage, res: ServerResponse) => {
      if (req.method !== 'POST') {
        res.statusCode = 405 // Method Not Allowed

        const responseError = {
          error: `Only POST is allowed, received ${req.method ?? 'unknown'}`,
        }

        res.setHeader('Allow', 'POST')

        res.end(JSON.stringify(responseError))

        return
      }

      const bodyPromise = new Promise<RequestBody>((resolve, reject) => {
        let body: any = '' // eslint-disable-line

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
          } catch (e: unknown) {
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

              const pkgResponse = await store.requestPackage(
                body.wantedDependency,
                body.options
              )

              if (pkgResponse.fetching) {
                filesPromises[body.msgId] = pkgResponse.fetching
              }

              res.end(JSON.stringify(pkgResponse.body))
            } catch (err: unknown) {
              res.end(
                JSON.stringify({
                  error: {
                    // @ts-ignore
                    ...err,
                  },
                })
              )
            }
            break
          }

          case '/fetchPackage': {
            try {
              body = await bodyPromise

              const pkgResponse = (store.fetchPackage as FetchPackageToStoreFunction)(body.options as any) // eslint-disable-line @typescript-eslint/no-explicit-any

              filesPromises[body.msgId] = pkgResponse.fetching

              res.end(
                JSON.stringify({ filesIndexFile: pkgResponse.filesIndexFile })
              )
            } catch (err: unknown) {
              res.end(
                JSON.stringify({
                  error: {
                    // @ts-ignore
                    ...err,
                  },
                })
              )
            }
            break
          }
          case '/packageFilesResponse': {
            body = await bodyPromise

            const filesResponse = await filesPromises[body.msgId]?.()

            delete filesPromises[body.msgId]

            res.end(JSON.stringify(filesResponse))

            break
          }

          case '/prune': {
            // Disable store pruning when a server is running
            res.statusCode = 403
            res.end()
            break
          }

          case '/importPackage': {
            const importPackageBody = (await bodyPromise) as any // eslint-disable-line @typescript-eslint/no-explicit-any

            await store.importPackage(
              importPackageBody.to,
              importPackageBody.opts
            )

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

            await lock(uploadBody.builtPkgLocation, async () =>
              store.upload(uploadBody.builtPkgLocation, uploadBody.opts)
            )

            res.end(JSON.stringify('OK'))

            break
          }

          case '/stop': {
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
          }

          default: {
            res.statusCode = 404

            const error = { error: `url "${req.url ?? ''}" does not match any route` }

            res.end(JSON.stringify(error))
          }
        }
      } catch (e: unknown) {
        res.statusCode = 503

        res.end(JSON.stringify(e))
      }
    }
  )

  let listener: Server

  if (opts.path) {
    listener = server.listen(opts.path)
  } else {
    listener = server.listen(opts.port, opts.hostname)
  }

  const waitForClose = new Promise<void>((resolve) =>
    listener.once('close', () => {
      resolve()
    })
  )

  return { close, waitForClose }

  async function close() {
    listener.close()
    return store.close()
  }
}
