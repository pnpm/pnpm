import { type ChildProcess } from 'child_process'
import chalk from 'chalk'
import crossSpawn from 'cross-spawn'
import { createEnv, pnpmBinLocation } from './execPnpm'
import { retryLoadJsonFile2 } from './retryLoadJsonFile'
import delay from 'delay'

interface ServerInstanceInfo {
  readonly connectionOptions: {
    readonly remotePrefix: string
  }
  readonly pid: number
  readonly pnpmVersion: string
}

// Polyfilling Symbol.asyncDispose for Jest.
//
// Copied with a few changes from https://devblogs.microsoft.com/typescript/announcing-typescript-5-2/#using-declarations-and-explicit-resource-management
if (Symbol.asyncDispose === undefined) {
  (Symbol as { asyncDispose?: symbol }).asyncDispose = Symbol('Symbol.asyncDispose')
}

const DEFAULT_EXEC_PNPM_TIMEOUT = 3 * 60 * 1000 // 3 minutes
const TIMEOUT_FOR_GRACEFUL_EXIT = 10 * 1000 // 10s

export class ServerTestingFramework implements AsyncDisposable {
  private readonly serverProcess: ChildProcess
  private serverOutput: string = ''

  constructor (serverStartArgs?: readonly string[]) {
    const serverProcess = this.spawnPnpm(['server', 'start', ...(serverStartArgs ?? [])])
    this.serverProcess = serverProcess

    const appendToServerOutput = (data: Buffer) => {
      this.serverOutput += data.toString()
    }

    // Intentionally interleaving stdout and stderr to mimic how this would look
    // in a user's console.
    this.serverProcess.stdout?.on('data', appendToServerOutput)
    this.serverProcess.stderr?.on('data', appendToServerOutput)
  }

  public async startup (serverJsonPath: string) {
    const { value, abortController } = retryLoadJsonFile2<ServerInstanceInfo>(serverJsonPath)

    return this.performServerAction({
      // It shouldn't take longer than 10s for the server to start up.
      timeout: 10_000,

      operation: () => value,
      cancel: async () => {
        abortController()
      },
      logs: () => '',
    })
  }

  public async exec (args: readonly string[], opts?: { timeout?: number }): Promise<void> {
    const proc = this.spawnPnpm(args)

    let clientOutput: string = ''

    function appendToClientOutput (data: Buffer) {
      clientOutput += data.toString()
    }

    proc.stdout?.on('data', appendToClientOutput)
    proc.stderr?.on('data', appendToClientOutput)

    return this.performServerAction({
      timeout: opts?.timeout,

      operation: () => {
        return new Promise((resolve, reject) => {
          proc.on('error', reject)

          proc.on('close', (code) => {
            if (code != null && code > 0) {
              reject(new Error(`Process exited with code ${code}`))
            } else {
              resolve()
            }
          })
        })
      },
      cancel: async () => {
        // Ask the process to exit politely and clean up its resources. On Windows
        // this will likely no-op since there is no SIGINT. The SIGTERM kill below
        // will stop the process in that case.
        proc.kill('SIGINT')

        await delay(TIMEOUT_FOR_GRACEFUL_EXIT)

        if (proc.exitCode !== null) {
          proc.kill()
        }
      },
      logs: () => clientOutput,
    })
  }

  private async performServerAction<T> (opts: {
    operation: () => Promise<T>
    cancel: () => Promise<void>
    logs: () => string
    timeout?: number
  }) {
    const timeout = opts.timeout ?? DEFAULT_EXEC_PNPM_TIMEOUT

    const timeoutSymbol = Symbol('timeout')
    let timeoutId: NodeJS.Timeout | undefined
    const timeoutPromise = new Promise((resolve) => {
      timeoutId = setTimeout(() => {
        resolve(timeoutSymbol)
      }, timeout)
    })

    let raceResult: Awaited<T> | typeof timeoutSymbol
    try {
      raceResult = await Promise.race([
        opts.operation(),
        timeoutPromise,
      ]) as Awaited<T> | typeof timeoutSymbol
    } catch (error: unknown) {
      throw new ServerTestExecError(error as string, this.serverOutput, opts.logs())
    } finally {
      clearTimeout(timeoutId)
    }

    if (raceResult === timeoutSymbol) {
      await opts.cancel()
      throw new ServerTestExecTimeoutError(timeout, this.serverOutput, '')
    }

    return raceResult
  }

  private spawnPnpm (
    args: readonly string[],
    opts?: {
      env?: Record<string, string>
      storeDir?: string
    }
  ): ChildProcess {
    return crossSpawn.spawn(process.execPath, [pnpmBinLocation, ...args], {
      env: {
        ...createEnv(opts),
        ...opts?.env,
      } as NodeJS.ProcessEnv,
    })
  }

  public async [Symbol.asyncDispose] (): Promise<void> {
    await this.exec(['server', 'stop'])

    if (this.serverProcess.exitCode === null) {
      this.serverProcess.kill('SIGTERM')
    }
  }
}

export class ServerTestExecError extends Error {
  /**
   * The entire server's output before the error. This will contain logs from
   * prior executions. The stdout and stderr streams are interleaved.
   */
  readonly serverOutput: string

  /**
   * The interleaved stdout and stderr of the pnpm exec during a server test.
   */
  readonly clientOutput: string

  constructor (message: string, serverOutput: string, clientOutput: string) {
    super(message)
    this.serverOutput = serverOutput
    this.clientOutput = clientOutput
  }
}

export class ServerTestExecTimeoutError extends ServerTestExecError {
  constructor (timeout: number, serverOutput: string, clientOutput: string) {
    super(`The running command did not exit after ${timeout}ms.`, serverOutput, clientOutput)
  }
}

expect.extend({
  async toBePassingServerTest (received: Promise<unknown>): Promise<jest.CustomMatcherResult> {
    try {
      await received
    } catch (error: unknown) {
      return {
        pass: false,
        message: () => {
          if (error instanceof ServerTestExecError) {
            return `\
${error.message}

${chalk.underline('Client log:')}
${error.clientOutput}

${chalk.underline('Server log:')}
${error.serverOutput}
`
          } else if (error instanceof Error) {
            return error.toString()
          }

          return error as string
        },
      }
    }

    return {
      pass: true,
      message: () => 'The client call succeeded.',
    }
  },
})

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace jest {
    interface Matchers<R> {
      toBePassingServerTest: () => Promise<R>
    }
  }
}
