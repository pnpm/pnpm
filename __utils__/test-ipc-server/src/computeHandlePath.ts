import crypto from 'node:crypto'
import os from 'node:os'
import path from 'node:path'

export function computeHandlePath (handle?: string) {
  const handleFilePath = handle != null
    ? path.resolve(handle)
    : path.join(os.tmpdir(), `${crypto.randomUUID()}.sock`)

  // Node.js and libuv do not yet support unix sockets on Windows.
  // https://github.com/libuv/libuv/issues/2537
  //
  // Until then, the best IPC alternative on Windows is a "named pipe", which
  // needs to be prefixed with '\\<server-name>\pipe'.
  // https://nodejs.org/api/net.html#identifying-paths-for-ipc-connections
  return os.platform() === 'win32'
    ? path.join('\\\\.\\pipe', 'pnpm-test-ipc-server', handleFilePath)
    : handleFilePath
}
