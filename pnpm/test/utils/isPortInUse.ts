import { createServer } from 'net'

export const isPortInUse = (port: number): Promise<boolean> => new Promise((resolve, reject) => {
  const server = createServer()
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  server.once('error', (err: any) => {
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
