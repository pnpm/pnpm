import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import loadJsonFile = require('load-json-file')
import {resolveStore, StoreController} from 'package-store'
import path = require('path')
import retry = require('retry')
import createStore from './createStore'
import runServerInBackground from './runServerInBackground'

export default async function (
  opts: {
    alwaysAuth?: boolean,
    registry?: string,
    rawNpmConfig: object,
    strictSsl?: boolean,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
    userAgent?: string,
    ignoreFile?: (filename: string) => boolean,
    offline?: boolean,
    lock?: boolean,
    lockStaleDuration?: number,
    networkConcurrency?: number,
    store?: string,
    prefix: string,
    useStoreServer?: boolean,
  },
): Promise<{
  ctrl: StoreController,
  path: string,
}> {
  const store = await resolveStore(opts.store, opts.prefix)
  try {
    const serverJson = await loadJsonFile(path.join(store, 'server.json'))
    logger.info('A store server is running. All store manipulations are delegated to it.')
    return {
      ctrl: await connectStoreController(serverJson.connectionOptions), // tslint:disable-line
      path: store,
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  if (opts.useStoreServer) {
    runServerInBackground()
    const operation = retry.operation()

    return new Promise<{
      ctrl: StoreController,
      path: string,
    }>((resolve, reject) => {
      operation.attempt(async (currentAttempt) => {
        try {
          const serverJson = await loadJsonFile(path.join(store, 'server.json'))
          logger.info('A store server has been started. To stop it, use \`pnpm server stop\`')
          resolve({
            ctrl: await connectStoreController(serverJson.connectionOptions), // tslint:disable-line
            path: store,
          })
          return
        } catch (err) {
          if (!operation.retry(err)) {
            reject(operation.mainError())
          }
        }
      })
    })
  }
  return await createStore(Object.assign(opts, {
    store,
  }))
}
