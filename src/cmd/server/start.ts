import logger from '@pnpm/logger'
import {createServer} from '@pnpm/server'
import storePath from '@pnpm/store-path'
import Diable = require('diable')
import getPort = require('get-port')
import fs = require('graceful-fs')
import isWindows = require('is-windows')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import onExit = require('signal-exit')
import writeJsonFile = require('write-json-file')
import createStore from '../../createStore'
import serverConnectionInfoDir from '../../serverConnectionInfoDir'
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
    store: await storePath(opts.prefix, opts.store),
  }))

  const connectionInfoDir = serverConnectionInfoDir(store.path)
  await mkdirp(connectionInfoDir)

  const protocol = opts.protocol || opts.port && 'tcp' || 'auto'
  const serverOptions = await getServerOptions(connectionInfoDir, {protocol, port: opts.port})
  const connectionOptions = {
    remotePrefix: serverOptions.path
      ? `http://unix:${serverOptions.path}:`
      : `http://${serverOptions.hostname}:${serverOptions.port}`,
  }
  const serverJsonPath = path.join(connectionInfoDir, 'server.json')
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
  connectionInfoDir: string,
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
      path: path.join(connectionInfoDir, 'socket'),
    }
  }
}
