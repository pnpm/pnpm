import type http from 'node:http'

export async function closeServer (server: http.Server): Promise<void> {
  server.closeAllConnections()
  await new Promise<void>((resolve, reject) => {
    server.close((err) => {
      if (err == null) {
        resolve()
      } else {
        reject(err)
      }
    })
  })
}
