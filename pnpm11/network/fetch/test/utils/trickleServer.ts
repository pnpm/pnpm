import { createServer, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'

export interface TestServer extends AsyncDisposable {
  url: string
}

export async function startServer (respond: (res: ServerResponse) => void): Promise<TestServer> {
  const server = createServer((_req, res) => {
    res.writeHead(200, { 'content-type': 'application/octet-stream' })
    respond(res)
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  const { port } = server.address() as AddressInfo
  return {
    url: `http://127.0.0.1:${port}/`,
    async [Symbol.asyncDispose] () {
      server.closeAllConnections()
      await new Promise<void>((resolve) => {
        server.close(() => {
          resolve()
        })
      })
    },
  }
}
