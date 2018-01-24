import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import delay = require('delay')
import loadJsonFile = require('load-json-file')
import {resolveStore} from 'package-store'
import path = require('path')
import processExists = require('process-exists')
import killcb = require('tree-kill')
import promisify = require('util.promisify')

const kill = promisify(killcb)

export default async (
  opts: {
    store?: string,
    prefix: string,
  },
) => {
  const store = await resolveStore(opts.store, opts.prefix)
  let serverJson: any | undefined // tslint:disable-line
  try {
    serverJson = await loadJsonFile(path.join(store, 'server.json'))
  } catch (err) {
    if (err.code !== 'ENOENT') {
      throw err
    } else {
      logger.info(`Nothing to stop. No server is running for the store at ${store}`)
      return
    }
  }
  const storeController = await connectStoreController(serverJson.connectionOptions)
  await storeController.stop()

  if (!await processExists(serverJson.pid) || await delay(5000) && !await processExists(serverJson.pid)) {
    logger.info('Server gracefully stopped')
    return
  }
  logger.warn('Graceful shutdown failed')
  await kill(serverJson.pid, 'SIGINT')
  logger.info('Server process terminated')
}
