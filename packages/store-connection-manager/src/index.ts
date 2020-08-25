import packageManager from '@pnpm/cli-meta'
import { Config } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { StoreController } from '@pnpm/package-store'
import { connectStoreController } from '@pnpm/server'
import storePath from '@pnpm/store-path'
import delay from 'delay'
import createNewStoreController, { CreateNewStoreControllerOptions } from './createNewStoreController'
import runServerInBackground from './runServerInBackground'
import serverConnectionInfoDir from './serverConnectionInfoDir'
import path = require('path')
import fs = require('mz/fs')

export { createNewStoreController, serverConnectionInfoDir }

export type CreateStoreControllerOptions = Omit<CreateNewStoreControllerOptions, 'storeDir'> & Pick<Config,
| 'storeDir'
| 'dir'
| 'useRunningStoreServer'
| 'useStoreServer'
>

export async function createOrConnectStoreControllerCached (
  storeControllerCache: Map<string, Promise<{ctrl: StoreController, dir: string}>>,
  opts: CreateStoreControllerOptions
) {
  const storeDir = await storePath(opts.dir, opts.storeDir)
  if (!storeControllerCache.has(storeDir)) {
    storeControllerCache.set(storeDir, createOrConnectStoreController(opts))
  }
  return await storeControllerCache.get(storeDir) as {ctrl: StoreController, dir: string}
}

export async function createOrConnectStoreController (
  opts: CreateStoreControllerOptions
): Promise<{
    ctrl: StoreController
    dir: string
  }> {
  const storeDir = await storePath(opts.dir, opts.storeDir)
  const connectionInfoDir = serverConnectionInfoDir(storeDir)
  const serverJsonPath = path.join(connectionInfoDir, 'server.json')
  let serverJson = await tryLoadServerJson({ serverJsonPath, shouldRetryOnNoent: false })
  if (serverJson !== null) {
    if (serverJson.pnpmVersion !== packageManager.version) {
      logger.warn({
        message: `The store server runs on pnpm v${serverJson.pnpmVersion}. It is recommended to connect with the same version (current is v${packageManager.version})`,
        prefix: opts.dir,
      })
    }
    logger.info({
      message: 'A store server is running. All store manipulations are delegated to it.',
      prefix: opts.dir,
    })
    return {
      ctrl: await connectStoreController(serverJson.connectionOptions), // eslint-disable-line
      dir: storeDir,
    }
  }
  if (opts.useRunningStoreServer) {
    throw new PnpmError('NO_STORE_SERVER', 'No store server is running.')
  }
  if (opts.useStoreServer) {
    runServerInBackground(storeDir)
    serverJson = await tryLoadServerJson({ serverJsonPath, shouldRetryOnNoent: true })
    logger.info({
      message: 'A store server has been started. To stop it, use `pnpm server stop`',
      prefix: opts.dir,
    })
    return {
      ctrl: await connectStoreController(serverJson!.connectionOptions), // eslint-disable-line
      dir: storeDir,
    }
  }
  return createNewStoreController(Object.assign(opts, {
    storeDir,
  }))
}

export async function tryLoadServerJson (
  options: {
    serverJsonPath: string
    shouldRetryOnNoent: boolean
  }
): Promise<null | {
    connectionOptions: {
      remotePrefix: string
    }
    pid: number
    pnpmVersion: string
  }> {
  let beforeFirstAttempt = true
  const startHRTime = process.hrtime()
  while (true) {
    if (!beforeFirstAttempt) {
      const elapsedHRTime = process.hrtime(startHRTime)
      // Time out after 10 seconds of waiting for the server to start, assuming something went wrong.
      // E.g. server got a SIGTERM or was otherwise abruptly terminated, server has a bug or a third
      // party is interfering.
      if (elapsedHRTime[0] >= 10) {
        // Delete the file in an attempt to recover from this bad state.
        try {
          await fs.unlink(options.serverJsonPath)
        } catch (error) {
          if (error.code !== 'ENOENT') {
            throw error
          }
          // Either the server.json was manually removed or another process already removed it.
        }
        return null
      }
      // Poll for server startup every 200 milliseconds.
      await delay(200)
    }
    beforeFirstAttempt = false
    let serverJsonStr
    try {
      serverJsonStr = await fs.readFile(options.serverJsonPath, 'utf8')
    } catch (error) {
      if (error.code !== 'ENOENT') {
        throw error
      }
      if (!options.shouldRetryOnNoent) {
        return null
      }
      continue
    }
    let serverJson
    try {
      serverJson = JSON.parse(serverJsonStr)
    } catch (error) {
      // Server is starting or server.json was modified by a third party.
      // We assume the best case and retry.
      continue
    }
    if (serverJson === null) {
      // Our server should never write null to server.json, even though it is valid json.
      throw new Error('server.json was modified by a third party')
    }
    return serverJson
  }
}
