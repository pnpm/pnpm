import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import logger from '@pnpm/logger'
import {createServer} from '@pnpm/server'
import fs = require('graceful-fs')
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
  const ipcConnectionPath = createIpcConnectionPath(strictOpts.store)
  const connectionOptions = {
    path: ipcConnectionPath,
  }
  const serverJsonPath = path.join(strictOpts.store, 'server.json')
  await writeJsonFile(serverJsonPath, {connectionOptions})
  const server = createServer(storeCtrl, connectionOptions)

  onExit(() => {
    server.close()
    fs.unlinkSync(serverJsonPath)
  })
}

function createIpcConnectionPath (fsPath: string) {
  fsPath = path.normalize(fsPath) + path.sep + 'socket'
  if (process.platform === 'win32') {
    return `\\\\.\\pipe\\${fsPath}`
  }
  return fsPath
}
