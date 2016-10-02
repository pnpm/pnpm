import path = require('path')
import crossSpawn = require('cross-spawn')

const pnpmBin = path.join(__dirname, '../../src/bin/pnpm.ts')

export default function (...args: string[]) {
  return new Promise((resolve, reject) => {
    const proc = crossSpawn.spawn('ts-node', [pnpmBin].concat(args), {stdio: 'inherit'})

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      resolve()
    })
  })
}
