import {RequestPackageFunction} from '@pnpm/package-requester'
import JsonSocket = require('json-socket')
import net = require('net')

export default function (
  requestPackage: RequestPackageFunction,
  opts: {
    port: number,
    hostname?: string,
  },
) {
  const server = net.createServer()
  server.listen(opts.port, opts.hostname);
  server.on('connection', (socket) => {
    const jsonSocket = new JsonSocket(socket)
    jsonSocket.on('message', async (message) => {
      const packageResponse = await requestPackage(message.wantedDependency, message.options)
      jsonSocket.sendMessage({
        action: `packageResponse:${message.msgId}`,
        body: packageResponse,
      }, (err) => err && console.error(err))

      if (!packageResponse.isLocal) {
        packageResponse.fetchingFiles.then((packageFilesResponse) => {
          jsonSocket.sendMessage({
            action: `packageFilesResponse:${message.msgId}`,
            body: packageFilesResponse,
          }, (err) => err && console.error(err))
        })

        packageResponse.fetchingManifest.then((manifestResponse) => {
          jsonSocket.sendMessage({
            action: `manifestResponse:${message.msgId}`,
            body: manifestResponse,
          }, (err) => err && console.error(err))
        })
      }
    })
  })

  return {
    close: () => server.close(),
  }
}
