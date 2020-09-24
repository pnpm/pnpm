import { promisify } from 'util'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { connectStoreController } from '@pnpm/server'
import { serverConnectionInfoDir, tryLoadServerJson } from '@pnpm/store-connection-manager'
import storePath from '@pnpm/store-path'
import delay from 'delay'
import path = require('path')
import processExists = require('process-exists')
import killcb = require('tree-kill')

const kill = promisify(killcb) as (pid: number, signal: string) => Promise<void>

export default async (
  opts: {
    storeDir?: string
    dir: string
  }
) => {
  const storeDir = await storePath(opts.dir, opts.storeDir)
  const connectionInfoDir = serverConnectionInfoDir(storeDir)
  const serverJson = await tryLoadServerJson({
    serverJsonPath: path.join(connectionInfoDir, 'server.json'),
    shouldRetryOnNoent: false,
  })
  if (serverJson === null) {
    globalInfo(`Nothing to stop. No server is running for the store at ${storeDir}`)
    return
  }
  const storeController = await connectStoreController(serverJson.connectionOptions)
  await storeController.stop()

  if (await serverGracefullyStops(serverJson.pid)) {
    globalInfo('Server gracefully stopped')
    return
  }
  globalWarn('Graceful shutdown failed')
  await kill(serverJson.pid, 'SIGINT')
  globalInfo('Server process terminated')
}

async function serverGracefullyStops (pid: number) {
  if (!await processExists(pid)) return true

  await delay(5000)

  return !await processExists(pid)
}
