import net from 'node:net'
import { promisify } from 'node:util'
import { computeHandlePath } from './computeHandlePath'

// Polyfilling Symbol.asyncDispose for Jest.
//
// Copied with a few changes from https://devblogs.microsoft.com/typescript/announcing-typescript-5-2/#using-declarations-and-explicit-resource-management
if (Symbol.asyncDispose === undefined) {
  (Symbol as { asyncDispose?: symbol }).asyncDispose = Symbol('Symbol.asyncDispose')
}
if (Symbol.dispose === undefined) {
  (Symbol as { dispose?: symbol }).dispose = Symbol('Symbol.dispose')
}

/**
 * A simple Inter-Process Communication (IPC) server written specifically for
 * usage in pnpm tests.
 *
 * It's a simple wrapper around Node.js's builtin IPC support. Messages sent to
 * the server are saved to a buffer that can be retrieved for assertions.
 */
export class TestIpcServer implements AsyncDisposable {
  private readonly server: net.Server
  private buffer = ''

  public readonly listenPath: string

  constructor (server: net.Server, listenPath: string) {
    this.server = server
    this.listenPath = listenPath

    server.on('connection', (client) => {
      client.on('data', data => {
        this.buffer += data.toString()
      })
    })
  }

  /**
   * Creates a new IPC server.
   *
   * The handle is expected to be a file system path. On Linux and macOS, a unix
   * socket is created at this path. On Windows, a named pipe is created using
   * the path as the name.
   */
  public static async listen (handle?: string): Promise<TestIpcServer> {
    const listenPath = computeHandlePath(handle)
    const server = net.createServer()
    const testIpcServer = new TestIpcServer(server, listenPath)

    return new Promise((resolve, reject) => {
      server.once('error', reject)

      server.listen(listenPath, () => {
        resolve(testIpcServer)
      })
    })
  }

  /**
   * Return the buffer of received messages.
   */
  public getBuffer (): string {
    return this.buffer
  }

  /**
   * Return the buffer as an array of strings split by the new line character.
   */
  public getLines (): string[] {
    return this.buffer === ''
      ? []
      : this.buffer.trim().split('\n')
  }

  /**
   * Reset the buffer to an empty string.
   */
  public clear (): void {
    this.buffer = ''
  }

  /**
   * Generates a shell script that can used as a package manifest "scripts"
   * entry. Exits after sending the message.
   */
  public sendLineScript (message: string): string {
    return `node -e "const c = require('net').connect('${JSON.stringify(this.listenPath).slice(1, -1)}', () => { c.write('${message}\\n'); c.end(); })"`
  }

  /**
   * Generates a shell script that can used as a package manifest "scripts"
   * entry. This script consumes its stdin and sends it to the server.
   */
  public generateSendStdinScript (): string {
    return `node -e "const c = require('net').connect('${JSON.stringify(this.listenPath).slice(1, -1)}', () => { process.stdin.pipe(c).on('end', () => { c.destroy(); }); })"`
  }

  public [Symbol.asyncDispose] = async (): Promise<void> => {
    const close = promisify(this.server.close).bind(this.server)
    await close()
  }
}

export const createTestIpcServer = TestIpcServer.listen
