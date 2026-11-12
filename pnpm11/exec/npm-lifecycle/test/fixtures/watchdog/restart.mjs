import childProcess from 'node:child_process'
import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import path from 'node:path'

const [directory, mode, replacement] = process.argv.slice(2)
const marker = path.join(directory, 'ran')

if (mode === 'script') {
  fs.writeFileSync(marker, 'ran')
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
  let watchdogProcess
  childProcess.spawn = (command, args, spawnOptions) => {
    const child = originalSpawn(command, args, spawnOptions)
    if (args.some((arg) => arg.includes("trap '' INT TERM HUP"))) watchdogProcess = child
    return child
  }
  syncBuiltinESMExports()
  await lifecycle({ name: 'warm', version: '1.0.0', scripts: { start: 'exit 0' } }, 'start', directory, options)
  // Keep automatic replacement pending until the launcher requests recovery.
  if (replacement === 'pending') watchdogProcess.removeAllListeners('exit')
  childProcess.spawn = (command, args, spawnOptions) => {
    if (args.some((arg) => arg.includes('restart.mjs') && arg.includes('script'))) {
      const { stdout } = childProcess.spawnSync('ps', ['-A', '-o', 'pid=', '-o', 'ppid=', '-o', 'args='], { encoding: 'utf8' })
      const watchdog = stdout.split('\n')
        .map((line) => line.trim().split(/\s+/))
        .find(([, parent, ...argv]) => Number(parent) === process.pid && argv.join(' ').includes("trap '' INT TERM HUP"))
      if (watchdog == null) throw new Error('No shared watchdog was started')
      process.kill(Number(watchdog[0]), 'SIGKILL')
      const deadline = Date.now() + 5_000
      while (Date.now() < deadline) {
        const { stdout: state } = childProcess.spawnSync('ps', ['-p', watchdog[0], '-o', 'stat='], { encoding: 'utf8' })
        if (!state.trim() || state.trim().startsWith('Z')) break
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 10)
      }
    }
    return originalSpawn(command, args, spawnOptions)
  }
  syncBuiltinESMExports()

  await lifecycle({
    name: 'after-watchdog-death',
    version: '1.0.0',
    scripts: { start: `${JSON.stringify(process.execPath)} ${JSON.stringify(import.meta.filename)} ${JSON.stringify(directory)} script` },
  }, 'start', directory, options)
}
