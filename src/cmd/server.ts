import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import logger from '@pnpm/logger'
import {createServer} from '@pnpm/server'
import getPort = require('get-port')
import fs = require('graceful-fs')
import isWindows = require('is-windows')
import mkdirp = require('mkdirp-promise')
import createStore from 'package-store'
import path = require('path')
import onExit = require('signal-exit')
import { PnpmOptions } from 'supi'
import extendOptions from 'supi/lib/api/extendOptions'
import writeJsonFile = require('write-json-file')

export default async (input: string[], opts: PnpmOptions) => {
  logger.warn('The store server is an experimental feature. Breaking changes may happen in non-major versions.')

  const strictOpts = await extendOptions(opts)

  const resolve = createResolver(strictOpts)
  const fetchers = createFetcher(strictOpts)
  const storeCtrl = await createStore(resolve, fetchers as {}, {
    lockStaleDuration: strictOpts.lockStaleDuration,
    locks: strictOpts.locks,
    networkConcurrency: strictOpts.networkConcurrency,
    store: strictOpts.store,
  })

  // the store folder will be needed because server will want to create a file there
  // for the IPC connection
  await mkdirp(strictOpts.store)

  const serverOptions = await getServerOptions(strictOpts.store)
  const connectionOptions = {
    remotePrefix: serverOptions.path
      ? `http://unix:${serverOptions.path}:`
      : `http://${serverOptions.hostname}:${serverOptions.port}`,
  }
  const serverJsonPath = path.join(strictOpts.store, 'server.json')
  await writeJsonFile(serverJsonPath, {connectionOptions})

  const server = createServer(storeCtrl, serverOptions)

  onExit(() => {
    server.close()
    fs.unlinkSync(serverJsonPath)
  })
}

async function getServerOptions (fsPath: string): Promise<{hostname?: string, port?: number, path?: string}> {
  if (isWindows()) {
    return {
      hostname: 'localhost',
      port: await getPort({port: 5813}),
    }
  }
  return {
    path: path.normalize(fsPath) + path.sep + 'socket',
  }
}
