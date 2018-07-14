import {storeLogger} from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import storePath from '@pnpm/store-path'
import delay = require('delay')
import path = require('path')
import processExists = require('process-exists')
import killcb = require('tree-kill')
import promisify = require('util.promisify')
import {tryLoadServerJson} from '../../createStoreController'
import serverConnectionInfoDir from '../../serverConnectionInfoDir'

const kill = promisify(killcb)

export default async (
  opts: {
    store?: string,
    prefix: string,
  },
) => {
  const store = await storePath(opts.prefix, opts.store)
  const connectionInfoDir = serverConnectionInfoDir(store)
  const serverJson = await tryLoadServerJson({
    serverJsonPath: path.join(connectionInfoDir, 'server.json'),
    shouldRetryOnNoent: false,
  })
  if (serverJson === null) {
    storeLogger.info(`Nothing to stop. No server is running for the store at ${store}`)
    return
  }
  const storeController = await connectStoreController(serverJson.connectionOptions)
  await storeController.stop()

  if (!await processExists(serverJson.pid) || await delay(5000) && !await processExists(serverJson.pid)) {
    storeLogger.info('Server gracefully stopped')
    return
  }
  storeLogger.warn('Graceful shutdown failed')
  await kill(serverJson.pid, 'SIGINT')
  storeLogger.info('Server process terminated')
}
