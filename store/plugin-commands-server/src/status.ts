import path from 'path'
import { type Config } from '@pnpm/config'
import { globalInfo } from '@pnpm/logger'
import { serverConnectionInfoDir, tryLoadServerJson } from '@pnpm/store-connection-manager'
import { getStorePath } from '@pnpm/store-path'

export async function status (
  opts: Pick<Config, 'dir' | 'pnpmHomeDir' | 'storeDir'>
): Promise<void> {
  const storeDir = await getStorePath({
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
    globalInfo(`No server is running for the store at ${storeDir}`)
    return
  }
  console.log(`store: ${storeDir}
process id: ${serverJson.pid}
remote prefix: ${serverJson.connectionOptions.remotePrefix}`)
}
