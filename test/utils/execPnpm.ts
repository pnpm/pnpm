import path = require('path')
import crossSpawn = require('cross-spawn')
import loadJsonFile = require('load-json-file')

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync(path.join(pkgRoot, 'package.json'))
const pnpmBin = path.join(pkgRoot, pnpmPkg.bin.pnpm)

export default function (...args: string[]): Promise<void>
export default async function () {
  const args = Array.prototype.slice.call(arguments)
  await new Promise((resolve, reject) => {
    const proc = crossSpawn.spawn('node', [pnpmBin].concat(args), {stdio: 'inherit'})

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      resolve()
    })
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
  return crossSpawn.sync('node', [pnpmBin].concat(args))
}
