import logger from '@pnpm/logger'
import {createServer} from '@pnpm/server'
import getPort = require('get-port')
import fs = require('graceful-fs')
import isWindows = require('is-windows')
import mkdirp = require('mkdirp-promise')
import {resolveStore} from 'package-store'
import path = require('path')
import onExit = require('signal-exit')
import writeJsonFile = require('write-json-file')
import createStore from '../createStore'
import { PnpmOptions } from '../types'

export default async (input: string[], opts: PnpmOptions) => {
  logger.warn('The store server is an experimental feature. Breaking changes may happen in non-major versions.')

  const store = await createStore(Object.assign(opts, {
    store: await resolveStore(opts.store, opts.prefix),
  }))

  // the store folder will be needed because server will want to create a file there
  // for the IPC connection
  await mkdirp(store.path)

  const serverOptions = await getServerOptions(store.path)
  const connectionOptions = {
    remotePrefix: serverOptions.path
      ? `http://unix:${serverOptions.path}:`
      : `http://${serverOptions.hostname}:${serverOptions.port}`,
  }
  const serverJsonPath = path.join(store.path, 'server.json')
  await writeJsonFile(serverJsonPath, {connectionOptions})

  const server = createServer(store.ctrl, serverOptions)

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
