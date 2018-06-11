import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import storePath from '@pnpm/store-path'
import delay = require('delay')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import {StoreController} from 'package-store'
import path = require('path')
import createStore from './createStore'
import packageManager from './pnpmPkgJson'
import runServerInBackground from './runServerInBackground'
import serverConnectionInfoDir from './serverConnectionInfoDir'

export default async function (
  opts: {
    alwaysAuth?: boolean,
    registry?: string,
    rawNpmConfig: object,
    strictSsl?: boolean,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
    userAgent?: string,
    ignoreFile?: (filename: string) => boolean,
    offline?: boolean,
    lock?: boolean,
    lockStaleDuration?: number,
    networkConcurrency?: number,
    store?: string,
    prefix: string,
    useStoreServer?: boolean,
  },
): Promise<{
  ctrl: StoreController,
  path: string,
}> {
  const store = await storePath(opts.prefix, opts.store)
  const connectionInfoDir = serverConnectionInfoDir(store)
  const serverJsonPath = path.join(connectionInfoDir, 'server.json')
  let serverJson = await tryLoadServerJson({ serverJsonPath, shouldRetryOnNoent: false })
  if (serverJson !== null) {
    if (serverJson.pnpmVersion !== packageManager.version) {
      const err = new Error(`The store server runs on pnpm v${serverJson.pnpmVersion}. The same pnpm version should be used to connect (current is v${packageManager.version})`)
      err['code'] = 'ERR_PNPM_INCOMPATIBLE_STORE_SERVER' // tslint:disable-line:no-string-literal
      throw err
    }
    logger.info('A store server is running. All store manipulations are delegated to it.')
    return {
      ctrl: await connectStoreController(serverJson.connectionOptions), // tslint:disable-line
      path: store,
    }
  }
  if (opts.useStoreServer) {
    runServerInBackground(store)
    serverJson = await tryLoadServerJson({ serverJsonPath, shouldRetryOnNoent: true })
    logger.info('A store server has been started. To stop it, use \`pnpm server stop\`')
    return {
      ctrl: await connectStoreController(serverJson.connectionOptions), // tslint:disable-line
      path: store,
    }
  }
  return await createStore(Object.assign(opts, {
    store,
  }))
}

export async function tryLoadServerJson (options: {
  serverJsonPath: string;
  shouldRetryOnNoent: boolean;
}) {
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
