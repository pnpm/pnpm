import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { ChildProcess as NodeChildProcess } from 'child_process'
import crossSpawn = require('cross-spawn')
import path = require('path')

const binDir = path.join(__dirname, '..', '..', 'bin')
const pnpmBinLocation = path.join(binDir, 'pnpm.js')
const pnpxBinLocation = path.join(binDir, 'pnpx.js')

export async function execPnpm (
  args: string[],
  opts?: {
    env: Object,
  }
): Promise<void> {
  await new Promise((resolve, reject) => {
    const proc = spawnPnpm(args, opts)

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      resolve()
    })
  })
}

export function spawnPnpm (
  args: string[],
  opts?: {
    env?: Object,
    storeDir?: string,
  }
): NodeChildProcess {
  return crossSpawn.spawn('node', [pnpmBinLocation, ...args], {
    env: {
      ...createEnv(opts),
      ...opts?.env,
    } as NodeJS.ProcessEnv,
    stdio: 'inherit',
  })
}

export async function execPnpx (args: string[]): Promise<void> {
  await new Promise((resolve, reject) => {
    const proc = spawnPnpx(args)

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      resolve()
    })
  })
}

export function spawnPnpx (args: string[], opts?: {storeDir?: string}): NodeChildProcess {
  return crossSpawn.spawn('node', [pnpxBinLocation, ...args], {
    env: createEnv(opts),
    stdio: 'inherit',
  })
}

export type ChildProcess = {
  status: number,
  stdout: Object,
  stderr: Object,
}

export function execPnpmSync (args: string[], opts?: { env: Object }): ChildProcess {
  return crossSpawn.sync('node', [pnpmBinLocation, ...args], {
    env: {
      ...createEnv(),
      ...opts?.env,
    } as NodeJS.ProcessEnv,
  }) as ChildProcess
}

export function execPnpxSync (args: string[]): ChildProcess {
  return crossSpawn.sync('node', [pnpxBinLocation, ...args], {
    env: createEnv(),
  }) as ChildProcess
}

function createEnv (opts?: {storeDir?: string}): NodeJS.ProcessEnv {
  const env = {
    npm_config_fetch_retries: '4',
    npm_config_hoist: 'true',
    npm_config_independent_leaves: 'false',
    npm_config_registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    npm_config_silent: 'true',
    npm_config_store_dir: opts && opts.storeDir || '../store',
    // Although this is the default value of verify-store-integrity (as of pnpm 1.38.0)
    // on CI servers we set it to `false`. That is why we set it back to true for the tests
    npm_config_verify_store_integrity: 'true',
  }
  for (let [key, value] of Object.entries(process.env)) {
    if (key.toLowerCase() === 'path' || key === 'COLORTERM' || key === 'APPDATA') {
      env[key] = value
    }
  }
  return env
}
