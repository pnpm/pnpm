import { createServer } from 'node:net'

export const isPortInUse = (port: number): Promise<boolean> => new Promise((resolve, reject) => {
  const server = createServer()
  server.once('error', (err: NodeJS.ErrnoException) => {
    if (err?.code !== 'EADDRINUSE') {
      reject(err); return
    }
    resolve(true)
  })
  server.once('listening', () => {
    server.once('close', () => {
      resolve(false)
    }).close()
  })
  server.listen(port)
})
