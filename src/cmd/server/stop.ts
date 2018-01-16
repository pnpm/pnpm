import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import loadJsonFile = require('load-json-file')
import {resolveStore} from 'package-store'
import path = require('path')

export default async (
  opts: {
    store?: string,
    prefix: string,
  },
) => {
  const store = await resolveStore(opts.store, opts.prefix)
  try {
    const serverJson = await loadJsonFile(path.join(store, 'server.json'))
    const storeController = await connectStoreController(serverJson.connectionOptions)
    await storeController.stop()
    return
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  logger.info(`Nothing to stop. No server is running for the store at ${store}`)
}
