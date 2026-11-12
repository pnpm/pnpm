import childProcess from 'node:child_process'
import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import path from 'node:path'

const [directory, mode, identifier = '0'] = process.argv.slice(2)
const marker = path.join(directory, `started-${identifier}.json`)

if (mode === 'script') {
  const descendant = childProcess.spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' })
  fs.writeFileSync(marker, JSON.stringify([process.pid, descendant.pid]))
  setInterval(() => {}, 1000)
} else {
  const count = Number(mode)
  const leaders = []
  const originalSpawn = childProcess.spawn
  childProcess.spawn = (command, args, options) => {
    const child = originalSpawn(command, args, options)
    if (args.some((arg) => arg.includes('startup.mjs') && arg.includes('script'))) {
      leaders.push(child.pid)
      if (leaders.length === count) {
        process.stdout.write(`${JSON.stringify(leaders)}\n`)
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 30_000)
      }
    }
    return child
  }
  syncBuiltinESMExports()

  const { lifecycle } = await import('../../../lib/index.js')
  const noop = () => {}
  await Promise.all(Array.from({ length: count }, (_, index) => lifecycle({
    name: `startup-window-${index}`,
    version: '1.0.0',
    scripts: { start: `${JSON.stringify(process.execPath)} ${JSON.stringify(import.meta.filename)} ${JSON.stringify(directory)} script ${index}` },
  }, 'start', directory, {
    dir: directory,
    stdio: 'ignore',
    unsafePerm: true,
    runConcurrently: true,
    scriptShell: identifier === 'relative' ? './script-shell' : undefined,
    ...(identifier === 'extended' ? { stdio: ['ignore', 'ignore', 'ignore', 'pipe'] } : {}),
    log: { info: noop, verbose: noop, silly: noop, warn: noop, pause: noop, resume: noop },
  })))
}
