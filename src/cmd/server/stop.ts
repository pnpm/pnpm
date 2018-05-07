import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import storePath from '@pnpm/store-path'
import delay = require('delay')
import loadJsonFile = require('load-json-file')
import path = require('path')
import processExists = require('process-exists')
import killcb = require('tree-kill')
import promisify = require('util.promisify')
import serverConnectionInfoDir from '../../serverConnectionInfoDir'

const kill = promisify(killcb)

export default async (
  opts: {
    store?: string,
    prefix: string,
  },
) => {
  const store = await storePath(opts.prefix, opts.store)
  let serverJson: any | undefined // tslint:disable-line
  try {
    const connectionInfoDir = serverConnectionInfoDir(store)
    serverJson = await loadJsonFile(path.join(connectionInfoDir, 'server.json'))
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
