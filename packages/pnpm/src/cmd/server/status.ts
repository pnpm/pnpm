import { globalInfo } from '@pnpm/logger'
import { serverConnectionInfoDir, tryLoadServerJson } from '@pnpm/store-connection-manager'
import storePath from '@pnpm/store-path'
import { stripIndents } from 'common-tags'
import path = require('path')
import { PnpmOptions } from '../../types'

export default async (
  opts: PnpmOptions,
) => {
  const storeDir = await storePath(opts.dir, opts.storeDir)
  const connectionInfoDir = serverConnectionInfoDir(storeDir)
  const serverJson = await tryLoadServerJson({
    serverJsonPath: path.join(connectionInfoDir, 'server.json'),
    shouldRetryOnNoent: false,
  })
  if (serverJson === null) {
    globalInfo(`No server is running for the store at ${storeDir}`)
    return
  }
  console.log(stripIndents`
    store: ${storeDir}
    process id: ${serverJson.pid}
    remote prefix: ${serverJson.connectionOptions.remotePrefix}
  `)
}
