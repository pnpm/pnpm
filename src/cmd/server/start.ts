import logger from '@pnpm/logger'
import {createServer} from '@pnpm/server'
import Diable = require('diable')
import getPort = require('get-port')
import fs = require('graceful-fs')
import isWindows = require('is-windows')
import mkdirp = require('mkdirp-promise')
import {resolveStore} from 'package-store'
import path = require('path')
import onExit = require('signal-exit')
import writeJsonFile = require('write-json-file')
import createStore from '../../createStore'
import { PnpmOptions } from '../../types'

export default async (
  opts: PnpmOptions & {
    background?: boolean,
    protocol?: 'auto' | 'tcp' | 'ipc',
    port?: number,
    ignoreStopRequests?: boolean,
    ignoreUploadRequests?: boolean,
  },
) => {
  if (opts.protocol === 'ipc' && opts.port) {
    throw new Error('Port cannot be selected when server communicates via IPC')
  }

  if (opts.background && !Diable.isDaemon()) {
    Diable()
  }

  const store = await createStore(Object.assign(opts, {
    store: await resolveStore(opts.store, opts.prefix),
  }))

  // the store folder will be needed because server will want to create a file there
  // for the IPC connection
  await mkdirp(store.path)

  const protocol = opts.protocol || opts.port && 'tcp' || 'auto'
  const serverOptions = await getServerOptions(store.path, {protocol, port: opts.port})
  const connectionOptions = {
    remotePrefix: serverOptions.path
      ? `http://unix:${serverOptions.path}:`
      : `http://${serverOptions.hostname}:${serverOptions.port}`,
  }
  const serverJsonPath = path.join(store.path, 'server.json')
  await writeJsonFile(serverJsonPath, {
    connectionOptions,
    pid: process.pid,
  })

  const server = createServer(store.ctrl, {
    ...serverOptions,
    ignoreStopRequests: opts.ignoreStopRequests,
    ignoreUploadRequests: opts.ignoreUploadRequests,
  })

  onExit(() => {
    server.close()
    fs.unlinkSync(serverJsonPath)
  })
}

async function getServerOptions (
  fsPath: string,
  opts: {
    protocol: 'auto' | 'tcp' | 'ipc',
    port?: number,
  },
): Promise<{hostname?: string, port?: number, path?: string}> {
  switch (opts.protocol) {
    case 'tcp':
      return await getTcpOptions()
    case 'ipc':
      if (isWindows()) {
        throw new Error('IPC protocol is not supported on Windows currently')
      }
      return getIpcOptions()
    case 'auto':
      if (isWindows()) {
        return await getTcpOptions()
      }
      return getIpcOptions()
    default:
      throw new Error(`Protocol ${opts.protocol} is not supported`)
  }

  async function getTcpOptions () {
    return {
      hostname: 'localhost',
      port: opts.port || await getPort({port: 5813}),
    }
  }

  function getIpcOptions () {
    return {
      path: path.normalize(fsPath) + path.sep + 'socket',
    }
  }
}
