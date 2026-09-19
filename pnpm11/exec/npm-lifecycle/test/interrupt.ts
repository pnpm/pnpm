import { spawn, spawnSync } from 'node:child_process'
import path from 'node:path'

import { expect, test } from '@jest/globals'

const fixture = path.join(import.meta.dirname, 'fixtures', 'interrupt')
const runScript = path.join(fixture, 'run.mjs')
const terminalScript = path.join(import.meta.dirname, '../../../__utils__/scripts/terminal.py')
const shutdownTimeout = 10_000
const skipOnWindows = process.platform === 'win32' ? test.skip : test

skipOnWindows('Ctrl+C in a terminal interrupts the child once', () => {
  const { stdout, status, error } = spawnSync('python3', [terminalScript, process.execPath, runScript], { encoding: 'utf8', timeout: shutdownTimeout })
  expect(error).toBeUndefined()
  expect(stdout).not.toMatch(/forced/)
  expect(stdout).toMatch(/shut down/)
  expect(status).toBe(0)
})

skipOnWindows('a SIGINT sent to a process without a terminal is relayed to the child', async () => {
  const { stdout, code } = await runWithoutTerminal('dev', 'SIGINT')
  expect(stdout).not.toMatch(/forced/)
  expect(stdout).toMatch(/shut down/)
  expect(code).toBe(0)
})

// A sh that stays the script's parent holds a relayed SIGINT until its child
// exits. pnpm signals the script's process group instead, and waits for it.
skipOnWindows('a SIGINT reaches a script behind a shell that stays its parent', async () => {
  const { stdout } = await runWithoutTerminal('dev-behind-shell', 'SIGINT')
  expect(stdout).not.toMatch(/forced/)
  expect(stdout).toMatch(/shut down/)
})

// A sh that stays the script's parent dies from SIGTERM at once, which is how
// a container runtime stops pnpm. The script still gets the signal through its
// process group, and pnpm waits for it to finish shutting down.
skipOnWindows('a SIGTERM reaches a script behind a shell that stays its parent', async () => {
  const { stdout } = await runWithoutTerminal('dev-behind-shell', 'SIGTERM')
  expect(stdout).toMatch(/shut down/)
})

/**
 * Runs the fixture's `script` in a session without a terminal, sends
 * `signal` to the runner once the script has started, and resolves with the
 * runner's output and exit code. The output is read until the pipe closes,
 * which is after every process holding it has exited.
 */
async function runWithoutTerminal (script: string, signal: NodeJS.Signals): Promise<{ stdout: string, code: number | null }> {
  const proc = spawn(process.execPath, [runScript, script], { detached: true, stdio: ['ignore', 'pipe', 'inherit'] })
  let stdout = ''
  proc.stdout.setEncoding('utf8')
  const exited = new Promise<number | null>((resolve) => {
    proc.on('close', resolve)
  })
  const killTimer = setTimeout(() => {
    proc.kill('SIGKILL')
  }, shutdownTimeout)
  const started = new Promise<void>((resolve) => {
    proc.stdout.on('data', (data: string) => {
      stdout += data
      if (stdout.includes('started')) resolve()
    })
  })
  await Promise.race([started, exited])
  proc.kill(signal)
  const code = await exited
  clearTimeout(killTimer)
  return { stdout, code }
}
