import { promisify } from 'util'
import path from 'path'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { connectStoreController } from '@pnpm/server'
import { serverConnectionInfoDir, tryLoadServerJson } from '@pnpm/store-connection-manager'
import storePath from '@pnpm/store-path'
import delay from 'delay'
import processExists from 'process-exists'
import killcb from 'tree-kill'

const kill = promisify(killcb) as (pid: number, signal: string) => Promise<void>

export default async (
  opts: {
    storeDir?: string
    dir: string
    pnpmHomeDir: string
  }
) => {
  const storeDir = await storePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
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
