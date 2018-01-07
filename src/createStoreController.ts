import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import loadJsonFile = require('load-json-file')
import {resolveStore} from 'package-store'
import path = require('path')
import createStore from './createStore'

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
  },
) {
  const store = await resolveStore(opts.store, opts.prefix)
  try {
    const serverJson = await loadJsonFile(path.join(store, 'server.json'))
    logger.info('A store service is running and will be used to download the needed packages')
    return {
      ctrl: await connectStoreController(serverJson.connectionOptions), // tslint:disable-line
      path: store,
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return await createStore(Object.assign(opts, {
    store,
  }))
}
