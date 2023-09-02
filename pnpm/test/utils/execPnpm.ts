import { type ChildProcess as NodeChildProcess, type StdioOptions } from 'child_process'
import path from 'path'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import isWindows from 'is-windows'
import crossSpawn from 'cross-spawn'

const binDir = path.join(__dirname, '../..', isWindows() ? 'dist' : 'bin')
const pnpmBinLocation = path.join(binDir, 'pnpm.cjs')
const pnpxBinLocation = path.join(__dirname, '../../bin/pnpx.cjs')

// The default timeout for tests is 4 minutes. Set a timeout for execPnpm calls
// for 3 minutes to make it more clear what specific part of a test is timing
// out.
const DEFAULT_EXEC_PNPM_TIMEOUT = 3 * 60 * 1000 // 3 minutes
const TIMEOUT_FOR_GRACEFUL_EXIT = 10 * 1000 // 10s

export async function execPnpm (
  args: string[],
  opts?: {
    env: Record<string, string>
    timeout?: number // timeout in ms
  }
): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const proc = spawnPnpm(args, opts)

    const timeout = opts?.timeout ?? DEFAULT_EXEC_PNPM_TIMEOUT
    const timeoutId = registerProcessTimeout(proc, timeout, reject)

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      clearTimeout(timeoutId)

      if (code > 0) {
        reject(new Error(`Exit code ${code}`))
      } else {
        resolve()
      }
    })
  })
}

export function spawnPnpm (
  args: string[],
  opts?: {
    env?: Record<string, string>
    storeDir?: string
  }
): NodeChildProcess {
  return crossSpawn.spawn(process.execPath, [pnpmBinLocation, ...args], {
    env: {
      ...createEnv(opts),
      ...opts?.env,
    } as NodeJS.ProcessEnv,
    stdio: 'inherit',
  })
}

export async function execPnpx (args: string[]): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const proc = spawnPnpx(args)

    const timeoutId = registerProcessTimeout(proc, DEFAULT_EXEC_PNPM_TIMEOUT, reject)

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      clearTimeout(timeoutId)

      if (code > 0) {
        reject(new Error(`Exit code ${code}`))
      } else {
        resolve()
      }
    })
  })
}

export function spawnPnpx (args: string[], opts?: { storeDir?: string }): NodeChildProcess {
  return crossSpawn.spawn(process.execPath, [pnpxBinLocation, ...args], {
    env: createEnv(opts),
    stdio: 'inherit',
  })
}

export interface ChildProcess {
  status: number
  stdout: { toString: () => string }
  stderr: { toString: () => string }
}

export function execPnpmSync (
  args: string[],
  opts?: {
    env: Record<string, string>
    stdio?: StdioOptions
    timeout?: number
  }
): ChildProcess {
  return crossSpawn.sync(process.execPath, [pnpmBinLocation, ...args], {
    env: {
      ...createEnv(),
      ...opts?.env,
    } as NodeJS.ProcessEnv,
    stdio: opts?.stdio,
    timeout: opts?.timeout ?? DEFAULT_EXEC_PNPM_TIMEOUT,
  }) as ChildProcess
}

export function execPnpxSync (
  args: string[],
  opts?: {
    env: Record<string, string>
    timeout?: number
  }
): ChildProcess {
  return crossSpawn.sync(process.execPath, [pnpxBinLocation, ...args], {
    env: {
      ...createEnv(),
      ...opts?.env,
    } as NodeJS.ProcessEnv,
    timeout: opts?.timeout ?? DEFAULT_EXEC_PNPM_TIMEOUT,
  }) as ChildProcess
}

function createEnv (opts?: { storeDir?: string }): NodeJS.ProcessEnv {
  const env = {
    npm_config_fetch_retries: '4',
    npm_config_hoist: 'true',
    npm_config_registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    npm_config_silent: 'true',
    npm_config_store_dir: opts?.storeDir ?? '../store',
    // Although this is the default value of verify-store-integrity (as of pnpm 1.38.0)
    // on CI servers we set it to `false`. That is why we set it back to true for the tests
    npm_config_verify_store_integrity: 'true',
  }
  for (const [key, value] of Object.entries(process.env)) {
    if (key.toLowerCase() === 'path' || key === 'COLORTERM' || key === 'APPDATA') {
      env[key] = value
    }
  }
  return env
}

function registerProcessTimeout (proc: NodeChildProcess, timeout: number, onTimeout: (reason: Error) => void) {
  return setTimeout(() => {
    onTimeout(new Error(`Command timed out after ${timeout}ms`))

    // Ask the process to exit politely and clean up its resources. On Windows
    // this will likely no-op since there is no SIGINT. The SIGTERM kill below
    // will stop the process in that case.
    proc.kill('SIGINT')

    setTimeout(() => {
      if (proc.exitCode != null) {
        proc.kill()
      }
    }, TIMEOUT_FOR_GRACEFUL_EXIT)
  }, timeout)
}
