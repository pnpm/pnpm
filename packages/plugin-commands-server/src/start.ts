import packageManager from '@pnpm/cli-meta'
import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { createServer } from '@pnpm/server'
import {
  createNewStoreController,
  CreateStoreControllerOptions,
  serverConnectionInfoDir,
} from '@pnpm/store-connection-manager'
import storePath from '@pnpm/store-path'
import path = require('path')
import Diable = require('@zkochan/diable')
import getPort = require('get-port')
import isWindows = require('is-windows')
import fs = require('mz/fs')
import onExit = require('signal-exit')

const storeServerLogger = logger('store-server')

export default async (
  opts: CreateStoreControllerOptions & {
    background?: boolean
    protocol?: 'auto' | 'tcp' | 'ipc'
    port?: number
    ignoreStopRequests?: boolean
    ignoreUploadRequests?: boolean
  }
) => {
  if (opts.protocol === 'ipc' && opts.port) {
    throw new Error('Port cannot be selected when server communicates via IPC')
  }
  if (opts.background && !Diable.isDaemon()) {
    Diable()
  }
  const storeDir = await storePath(opts.dir, opts.storeDir)
  const connectionInfoDir = serverConnectionInfoDir(storeDir)
  const serverJsonPath = path.join(connectionInfoDir, 'server.json')
  await fs.mkdir(connectionInfoDir, { recursive: true })

  // Open server.json with exclusive write access to ensure only one process can successfully
  // start the server. Note: NFS does not support exclusive writing, but do we really care?
  // Source: https://github.com/moxystudio/node-proper-lockfile#user-content-comparison
  let fd: number|null
  try {
    fd = await fs.open(serverJsonPath, 'wx')
  } catch (error) {
    if (error.code !== 'EEXIST') {
      throw error
    }
    throw new PnpmError('SERVER_MANIFEST_LOCKED', `Canceling startup of server (pid ${process.pid}) because another process got exclusive access to server.json`)
  }
  let server: null|{close: () => Promise<void>} = null
  onExit(() => {
    if (server !== null) {
      // Note that server.close returns a Promise, but we cannot wait for it because we may be
      // inside the 'exit' even of process.
      server.close() // eslint-disable-line @typescript-eslint/no-floating-promises
    }
    if (fd !== null) {
      try {
        fs.closeSync(fd)
      } catch (error) {
        storeServerLogger.error(error, 'Got error while closing file descriptor of server.json, but the process is already exiting')
      }
    }
    try {
      fs.unlinkSync(serverJsonPath)
    } catch (error) {
      if (error.code !== 'ENOENT') {
        storeServerLogger.error(error, 'Got error unlinking server.json, but the process is already exiting')
      }
    }
  })
  const store = await createNewStoreController(Object.assign(opts, {
    storeDir,
  }))
  const protocol = opts.protocol ?? (opts.port ? 'tcp' : 'auto')
  const serverOptions = await getServerOptions(connectionInfoDir, { protocol, port: opts.port })
  const connectionOptions = {
    remotePrefix: serverOptions.path != null
      ? `http://unix:${serverOptions.path}:`
      : `http://${serverOptions.hostname!}:${serverOptions.port!}`,
  }
  server = createServer(store.ctrl, {
    ...serverOptions,
    ignoreStopRequests: opts.ignoreStopRequests,
    ignoreUploadRequests: opts.ignoreUploadRequests,
  })
  // Make sure to populate server.json after the server has started, so clients know that the server is
  // listening if a server.json with valid JSON content exists.
  const serverJson = {
    connectionOptions,
    pid: process.pid,
    pnpmVersion: packageManager.version,
  }
  const serverJsonStr = JSON.stringify(serverJson, undefined, 2) // undefined and 2 are for formatting.
  const serverJsonBuffer = Buffer.from(serverJsonStr, 'utf8')
  // fs.write on NodeJS 4 requires the parameters offset and length to be set:
  // https://nodejs.org/docs/latest-v4.x/api/fs.html#fs_fs_write_fd_buffer_offset_length_position_callback
  await fs.write(fd, serverJsonBuffer, 0, serverJsonBuffer.byteLength)

  const fdForClose = fd
  // Set fd to null so we only attempt to close it once.
  fd = null
  await fs.close(fdForClose)
}

async function getServerOptions (
  connectionInfoDir: string,
  opts: {
    protocol: 'auto' | 'tcp' | 'ipc'
    port?: number
  }
): Promise<(
    {
      hostname: string
      port: number
    } | { path: string }
  ) & { hostname?: string, port?: number, path?: string }> {
  switch (opts.protocol) {
  case 'tcp':
    return getTcpOptions()
  case 'ipc':
    if (isWindows()) {
      throw new Error('IPC protocol is not supported on Windows currently')
    }
    return getIpcOptions()
  case 'auto':
    if (isWindows()) {
      return getTcpOptions()
    }
    return getIpcOptions()
  default:
    throw new Error(`Protocol ${opts.protocol as string} is not supported`)
  }

  async function getTcpOptions () {
    return {
      hostname: 'localhost',
      port: opts.port || await getPort({ port: 5813 }), // eslint-disable-line
    }
  }

  function getIpcOptions () {
    return {
      path: path.join(connectionInfoDir, 'socket'),
    }
  }
}
