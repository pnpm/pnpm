import logger from '@pnpm/logger'
import storePath from '@pnpm/store-path'
import {stripIndents} from 'common-tags'
import path = require('path')
import {tryLoadServerJson} from '../../createStoreController'
import serverConnectionInfoDir from '../../serverConnectionInfoDir'
import { PnpmOptions } from '../../types'

export default async (
  opts: PnpmOptions,
) => {
  const store = await storePath(opts.prefix, opts.store)
  const connectionInfoDir = serverConnectionInfoDir(store)
  const serverJson = await tryLoadServerJson({
    serverJsonPath: path.join(connectionInfoDir, 'server.json'),
    shouldRetryOnNoent: false,
  })
  if (serverJson === null) {
    logger.info({
      message: `No server is running for the store at ${store}`,
      prefix: opts.prefix,
    })
    return
  }
  console.log(stripIndents`
    store: ${store}
    process id: ${serverJson.pid}
    remote prefix: ${serverJson.connectionOptions.remotePrefix}
  `)
}
