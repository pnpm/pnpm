import path = require('path')
import crossSpawn = require('cross-spawn')

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

export function spawn (args: string[]) {
  return crossSpawn.spawn('pnpm', args, {
    env: createEnv(),
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
  return crossSpawn.sync('pnpm', args, {
    env: createEnv(),
  })
}

function createEnv () {
  const _ = Object.assign({}, process.env, {
    npm_config_registry: 'http://localhost:4873/',
    npm_config_store: '../store',
    npm_config_silent: 'true',
  })
  return _
}
