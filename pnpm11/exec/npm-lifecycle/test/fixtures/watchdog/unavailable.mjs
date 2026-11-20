import childProcess from 'node:child_process'
import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import path from 'node:path'

const [directory, mode, previous] = process.argv.slice(2)

if (mode === 'script') {
  fs.writeFileSync(path.join(directory, 'ran'), 'ran')
} else {
  const { lifecycle } = await import('../../../lib/index.js')
  const noop = () => {}
  const options = {
    dir: directory,
    stdio: 'ignore',
    unsafePerm: true,
    log: { info: noop, verbose: noop, silly: noop, warn: noop, pause: noop, resume: noop },
  }
  const originalSpawn = childProcess.spawn
  const isWatchdog = (args) => args.some((arg) => arg.includes("trap '' INT TERM HUP"))
  if (previous === 'destroyed') {
    let watchdogProcess
    childProcess.spawn = (command, args, spawnOptions) => {
      const child = originalSpawn(command, args, spawnOptions)
      if (isWatchdog(args)) watchdogProcess = child
      return child
    }
    syncBuiltinESMExports()
    await lifecycle({ name: 'warm', version: '1.0.0', scripts: { start: 'exit 0' } }, 'start', directory, options)
    watchdogProcess.removeAllListeners('exit')
    watchdogProcess.stdin.destroy()
  }
  childProcess.spawn = (command, args, spawnOptions) => originalSpawn(
    isWatchdog(args) ? path.join(directory, 'missing-sh') : command,
    args,
    spawnOptions
  )
  syncBuiltinESMExports()
  await lifecycle({
    name: 'watchdog-unavailable',
    version: '1.0.0',
    scripts: { start: `${JSON.stringify(process.execPath)} ${JSON.stringify(import.meta.filename)} ${JSON.stringify(directory)} script` },
  }, 'start', directory, options)
}
