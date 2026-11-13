import childProcess from 'node:child_process'
import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import path from 'node:path'

const [directory, mode] = process.argv.slice(2)

if (mode === 'script') {
  fs.writeFileSync(path.join(directory, 'ran'), 'ran')
} else {
  const originalSpawn = childProcess.spawn
  childProcess.spawn = (command, args, spawnOptions) => originalSpawn(
    args.some((arg) => arg.includes("trap '' INT TERM HUP")) ? path.join(directory, 'missing-sh') : command,
    args,
    spawnOptions
  )
  syncBuiltinESMExports()
  const { lifecycle } = await import('../../../lib/index.js')
  const noop = () => {}
  await lifecycle({
    name: 'watchdog-unavailable',
    version: '1.0.0',
    scripts: { start: `${JSON.stringify(process.execPath)} ${JSON.stringify(import.meta.filename)} ${JSON.stringify(directory)} script` },
  }, 'start', directory, {
    dir: directory,
    stdio: 'ignore',
    unsafePerm: true,
    log: { info: noop, verbose: noop, silly: noop, warn: noop, pause: noop, resume: noop },
  })
}
