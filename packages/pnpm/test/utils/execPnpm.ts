import crossSpawn = require('cross-spawn')
import path = require('path')

const binDir = path.join(__dirname, '..', '..', 'bin')
const pnpmBinLocation = path.join(binDir, 'pnpm.js')
const pnpxBinLocation = path.join(binDir, 'pnpx.js')

export async function execPnpm (...args: string[]): Promise<void> {
  await new Promise((resolve, reject) => {
    const proc = spawn(args)

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      resolve()
    })
  })
}

export function spawn (args: string[], opts?: {storeDir?: string}) {
  return crossSpawn.spawn('node', [pnpmBinLocation, ...args], {
    env: createEnv(opts),
    stdio: 'inherit',
  })
}

export async function execPnpx (...args: string[]): Promise<void> {
  await new Promise((resolve, reject) => {
    const proc = spawn(args)

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      resolve()
    })
  })
}

export function spawnPnpx (args: string[], opts?: {storeDir?: string}) {
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

export function sync (...args: string[]): ChildProcess {
  return crossSpawn.sync('node', [pnpmBinLocation, ...args], {
    env: createEnv(),
  })
}

export function spawnPnpxSync (...args: string[]): ChildProcess {
  return crossSpawn.sync('node', [pnpxBinLocation, ...args], {
    env: createEnv(),
  })
}

function createEnv (opts?: {storeDir?: string}) {
  const _ = {
    ...process.env,
    npm_config_fetch_retries: 4,
    npm_config_independent_leaves: false,
    npm_config_registry: 'http://localhost:4873/',
    npm_config_silent: 'true',
    npm_config_store: opts && opts.storeDir || '../store',
    // Although this is the default value of verify-store-integrity (as of pnpm 1.38.0)
    // on CI servers we set it to `false`. That is why we set it back to true for the tests
    npm_config_verify_store_integrity: 'true',
  } as any // tslint:disable-line:no-any
  delete _.npm_config_link_workspace_packages
  delete _.npm_config_save_exact
  delete _.npm_config_shared_workspace_lockfile
  delete _.npm_config_workspace_concurrency
  delete _.npm_config_use_beta_cli
  return _
}
