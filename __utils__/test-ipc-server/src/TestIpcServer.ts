import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { promisify, stripVTControlCharacters } from 'node:util'

import { computeHandlePath } from './computeHandlePath.js'

// Polyfilling Symbol.asyncDispose for Jest.
//
// Copied with a few changes from https://devblogs.microsoft.com/typescript/announcing-typescript-5-2/#using-declarations-and-explicit-resource-management
if (Symbol.asyncDispose === undefined) {
  (Symbol as { asyncDispose?: symbol }).asyncDispose = Symbol('Symbol.asyncDispose')
}
if (Symbol.dispose === undefined) {
  (Symbol as { dispose?: symbol }).dispose = Symbol('Symbol.dispose')
}

// The helper scripts only ever invoke node with arguments passed via argv, so
// the IPC path never has to be embedded into JavaScript source or shell
// metacharacters. The script source is therefore completely static.
const STDIN_HELPER_SOURCE = `const net = require('node:net')
const target = process.argv[2]
const c = net.connect(target, () => {
  process.stdin.pipe(c).on('end', () => { c.destroy() })
})
`
const LINE_HELPER_SOURCE = `const net = require('node:net')
const target = process.argv[2]
const message = process.argv[3]
const c = net.connect(target, () => {
  c.write(message + '\\n')
  c.end()
})
`

/**
 * A simple Inter-Process Communication (IPC) server written specifically for
 * usage in pnpm tests.
 *
 * It's a simple wrapper around Node.js's builtin IPC support. Messages sent to
 * the server are saved to a buffer that can be retrieved for assertions.
 */
export class TestIpcServer implements AsyncDisposable {
  private readonly server: net.Server
  private readonly helperDir: string
  private readonly stdinHelperPath: string
  private readonly lineHelperPath: string
  private buffer = ''

  public readonly listenPath: string

  constructor (
    server: net.Server,
    listenPath: string,
    helpers: { dir: string, stdinHelperPath: string, lineHelperPath: string }
  ) {
    this.server = server
    this.listenPath = listenPath
    this.helperDir = helpers.dir
    this.stdinHelperPath = helpers.stdinHelperPath
    this.lineHelperPath = helpers.lineHelperPath

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
    const helperDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-test-ipc-'))
    const stdinHelperPath = path.join(helperDir, 'stdin.cjs')
    const lineHelperPath = path.join(helperDir, 'line.cjs')
    await Promise.all([
      fs.promises.writeFile(stdinHelperPath, STDIN_HELPER_SOURCE),
      fs.promises.writeFile(lineHelperPath, LINE_HELPER_SOURCE),
    ])
    const testIpcServer = new TestIpcServer(server, listenPath, {
      dir: helperDir,
      stdinHelperPath,
      lineHelperPath,
    })

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
   * VT control sequences are removed
   */
  public getLines (): string[] {
    return this.buffer === ''
      ? []
      : stripVTControlCharacters(this.buffer).trim().split('\n')
  }

  /**
   * Reset the buffer to an empty string.
   */
  public clear (): void {
    this.buffer = ''
  }

  /**
   * Generates a shell script that can be used as a package manifest "scripts"
   * entry. Exits after sending the message.
   *
   * Throws if `message` contains characters outside the allowlist enforced by
   * `quoteShellArg` (alphanumerics plus ``_ - . / \\ : @ space + = ,``). All
   * existing call sites pass short ASCII identifiers, so the constraint is
   * satisfied by construction.
   */
  public sendLineScript (message: string): string {
    return `node ${quoteShellArg(this.lineHelperPath)} ${quoteShellArg(this.listenPath)} ${quoteShellArg(message)}`
  }

  /**
   * Generates a shell script that can be used as a package manifest "scripts"
   * entry. This script consumes its stdin and sends it to the server.
   *
   * Throws if the server's `listenPath` contains characters outside the
   * allowlist enforced by `quoteShellArg`. The path is computed from
   * `os.tmpdir()` (or a Windows named-pipe prefix) and a random UUID, so the
   * constraint is satisfied by construction.
   */
  public generateSendStdinScript (): string {
    return `node ${quoteShellArg(this.stdinHelperPath)} ${quoteShellArg(this.listenPath)}`
  }

  public [Symbol.asyncDispose] = async (): Promise<void> => {
    const close = promisify(this.server.close).bind(this.server)
    await close()
    await fs.promises.rm(this.helperDir, { recursive: true, force: true })
  }
}

export const createTestIpcServer = TestIpcServer.listen

/**
 * Wrap an argument for inclusion in a shell command. The argument must contain
 * only a restricted set of characters known to be safe in both POSIX and
 * Windows command interpreters when surrounded by double quotes (alphanumerics
 * plus `_ - . / \\ : @ space + = ,`). A trailing backslash is rejected because
 * under both shells `\\"` consumes the closing quote, which would break the
 * command line.
 *
 * Throws when the argument contains a character outside this allowlist. All
 * arguments produced internally (helper-script paths, the listen-path computed
 * from `os.tmpdir()`/a named-pipe prefix, and test messages) satisfy this
 * constraint by construction.
 */
function quoteShellArg (arg: string): string {
  // Anchored allowlist — CodeQL recognizes this as a sanitization barrier for
  // shell-injection sinks. The `endsWith('\\')` check rules out the one
  // remaining ambiguous case the allowlist allows.
  if (arg.length === 0 || !/^[\w\-./\\:@ +=,]+$/.test(arg) || arg.endsWith('\\')) {
    throw new Error(`Unsupported character in shell argument: ${JSON.stringify(arg)}`)
  }
  return `"${arg}"`
}
