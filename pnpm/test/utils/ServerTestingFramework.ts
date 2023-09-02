import { type ChildProcess } from 'child_process'
import crossSpawn from 'cross-spawn'
import { createEnv, pnpmBinLocation } from './execPnpm'

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

  constructor (serverStartArgs?: readonly string[]) {
    const serverProcess = this.spawnPnpm(['server', 'start', ...(serverStartArgs ?? [])])
    this.serverProcess = serverProcess
  }

  public async exec (args: readonly string[], opts?: { timeout?: number }): Promise<void> {
    // Store the server's stderr and stdout for debugging if the process fails.
    let serverOutput: string = ''

    function appendToServerOutput (data: Buffer) {
      serverOutput += data.toString()
    }

    // Intentionally interleaving stdout and stderr to mimic how this would look
    // in a user's console.
    this.serverProcess.stdout?.on('data', appendToServerOutput)
    this.serverProcess.stderr?.on('data', appendToServerOutput)

    await new Promise<void>((resolve, reject) => {
      const proc = this.spawnPnpm(args)

      let processStartError: Error | undefined
      proc.on('error', (error) => {
        processStartError = error
      })

      let clientOutput: string = ''

      function appendToClientOutput (data: Buffer) {
        clientOutput += data.toString()
      }

      proc.stdout?.on('data', appendToClientOutput)
      proc.stderr?.on('data', appendToClientOutput)

      let didCommandTimeOut = false
      const commandTimeoutId = setTimeout(() => {
        didCommandTimeOut = true

        // Ask the process to exit politely and clean up its resources. On Windows
        // this will likely no-op since there is no SIGINT. The SIGTERM kill below
        // will stop the process in that case.
        proc.kill('SIGINT')

        setTimeout(() => {
          if (proc.exitCode !== null) {
            proc.kill()
          }
        }, TIMEOUT_FOR_GRACEFUL_EXIT)
      }, opts?.timeout ?? DEFAULT_EXEC_PNPM_TIMEOUT)

      proc.on('close', (code: number) => {
        this.serverProcess.stdout?.removeListener('data', appendToServerOutput)
        this.serverProcess.stderr?.removeListener('data', appendToServerOutput)
        clearInterval(commandTimeoutId)

        if (processStartError !== undefined) {
          reject(processStartError)
        }

        if (code > 0 || didCommandTimeOut) {
          reject(new Error(`
Exit code ${code}
didCommandTimeOut: ${didCommandTimeOut}

Server Output:
${serverOutput}

Command Output:
${clientOutput}
`))
        } else {
          resolve()
        }
      })
    })
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
    if (this.serverProcess.exitCode !== null) {
      return
    }

    this.serverProcess.kill('SIGINT')

    setTimeout(() => {
      if (this.serverProcess.exitCode !== null) {
        this.serverProcess.kill('SIGTERM')
      }
    }, 10_000).unref()

    await new Promise((resolve) => {
      this.serverProcess.once('close', resolve)
    })
  }
}
