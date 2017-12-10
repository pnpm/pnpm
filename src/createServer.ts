import {
  RequestPackageFunction,
  RequestPackageOptions,
  WantedDependency,
} from '@pnpm/package-requester'
import JsonSocket = require('json-socket')
import net = require('net')
import {StoreController} from 'package-store'

export default function (
  store: StoreController,
  opts: {
    port: number,
    hostname?: string,
  },
) {
  const server = net.createServer()
  server.listen(opts.port, opts.hostname);
  server.on('connection', (socket) => {
    const jsonSocket = new JsonSocket(socket)
    const requestPackage = requestPackageWithCtx.bind(null, {jsonSocket, store})

    jsonSocket.on('message', async (message) => {
      switch (message.action) {
        case 'requestPackage': {
          await requestPackage(message.msgId, message.args[0], message.args[1])
          return
        }
        case 'prune': {
          await store.prune()
          return
        }
        case 'updateConnections': {
          await store.updateConnections(message.args[0], message.args[1])
          return
        }
        case 'saveState':
        case 'saveStateAndClose': {
          await store.saveState()
          return
        }
      }
    })
  })

  return {
    close: () => server.close(),
  }
}

async function requestPackageWithCtx (
  ctx: {
    jsonSocket: JsonSocket,
    store: StoreController,
  },
  msgId: string,
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
) {
  const packageResponse = await ctx.store.requestPackage(wantedDependency, options)
  ctx.jsonSocket.sendMessage({
    action: `packageResponse:${msgId}`,
    body: packageResponse,
  }, (err) => err && console.error(err))

  if (!packageResponse.isLocal) {
    packageResponse.fetchingFiles.then((packageFilesResponse) => {
      ctx.jsonSocket.sendMessage({
        action: `packageFilesResponse:${msgId}`,
        body: packageFilesResponse,
      }, (err) => err && console.error(err))
    })

    packageResponse.fetchingManifest.then((manifestResponse) => {
      ctx.jsonSocket.sendMessage({
        action: `manifestResponse:${msgId}`,
        body: manifestResponse,
      }, (err) => err && console.error(err))
    })
  }
}
