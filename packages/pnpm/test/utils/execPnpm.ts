import crossSpawn = require('cross-spawn')
import path = require('path')

const pnpmBinLocation = path.join(__dirname, '..', '..', 'bin', 'pnpm.js')

export default function (...args: string[]): Promise<void>
export default async function () {
  const args = Array.prototype.slice.call(arguments)
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

export type ChildProcess = {
  status: number,
  stdout: Object,
  stderr: Object,
}

export function sync (...args: string[]): ChildProcess
export function sync (): ChildProcess {
  const args = Array.prototype.slice.call(arguments)
  return crossSpawn.sync('node', [pnpmBinLocation, ...args], {
    env: createEnv(),
  })
}

function createEnv (opts?: {storeDir?: string}) {
  const _ = Object.assign({}, process.env, {
    npm_config_registry: 'http://localhost:4873/',
    npm_config_store: opts && opts.storeDir || '../store',
    npm_config_silent: 'true',
    // Although this is the default value of verify-store-integrity (as of pnpm 1.38.0)
    // on CI servers we set it to `false`. That is why we set it back to true for the tests
    npm_config_verify_store_integrity: 'true',
  })
  delete _.npm_config_link_workspace_packages
  delete _.npm_config_save_exact
  delete _.npm_config_shared_workspace_shrinkwrap
  return _
}
