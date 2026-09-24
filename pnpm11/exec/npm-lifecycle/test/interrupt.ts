import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { endsWithin, killProcessGroup } from '@pnpm/prepare'

const fixture = path.join(import.meta.dirname, 'fixtures', 'interrupt')
const runScript = path.join(fixture, 'run.mjs')
const terminalScript = path.join(import.meta.dirname, '../../../__utils__/scripts/terminal.py')
const shutdownTimeout = 10_000
const skipOnWindows = process.platform === 'win32' ? test.skip : test

const markers = ['started.txt', 'shut-down.txt', 'forced.txt'].map((name) => path.join(fixture, name))

afterEach(() => {
  for (const marker of markers) {
    fs.rmSync(marker, { force: true })
  }
})

skipOnWindows('Ctrl+C in a terminal interrupts the child once', () => {
  const { status, error } = spawnSync('python3', [terminalScript, process.execPath, runScript], { encoding: 'utf8', timeout: shutdownTimeout })
  expect(error).toBeUndefined()
  expect(fs.existsSync(markers[2])).toBe(false)
  expect(fs.existsSync(markers[1])).toBe(true)
  expect(status).toBe(0)
})

skipOnWindows('a SIGINT sent to a process without a terminal is relayed to the child', async () => {
  const { shutDownBeforeExit, code } = await runWithoutTerminal('dev', 'SIGINT')
  expect(fs.existsSync(markers[2])).toBe(false)
  expect(shutDownBeforeExit).toBe(true)
  expect(code).toBe(0)
})

// A sh that stays the script's parent holds a relayed SIGINT until its child
// exits. pnpm signals the script's process group instead, and waits for it.
skipOnWindows('a SIGINT reaches a script behind a shell that stays its parent', async () => {
  const { shutDownBeforeExit } = await runWithoutTerminal('dev-behind-shell', 'SIGINT')
  expect(fs.existsSync(markers[2])).toBe(false)
  expect(shutDownBeforeExit).toBe(true)
})

// A sh that stays the script's parent dies from SIGTERM at once, which is how
// a container runtime stops pnpm. The script still gets the signal through its
// process group, and pnpm waits for it to finish shutting down.
skipOnWindows('a SIGTERM reaches a script behind a shell that stays its parent', async () => {
  const { shutDownBeforeExit } = await runWithoutTerminal('dev-behind-shell', 'SIGTERM')
  expect(shutDownBeforeExit).toBe(true)
})

// A tool that starts the runner detached and stops it by killing its process
// group, as Playwright's webServer does, reaches the runner but not a script
// in a group of its own. The script must still end with the runner: a survivor
// keeps the tool's output pipes open, and the tool waits on them for ever
// (https://github.com/pnpm/pnpm/issues/15555).
skipOnWindows('killing the runner\'s process group kills the script behind its shell too', async () => {
  const proc = spawn(process.execPath, [runScript, 'dev-behind-shell'], { cwd: fixture, detached: true, stdio: ['ignore', 'pipe', 'inherit'] })
  const closed = new Promise<void>((resolve) => {
    proc.on('close', () => {
      resolve()
    })
  })
  proc.stdout.resume()
  let script: number | undefined
  try {
    await waitForFile(markers[0])
    script = Number(fs.readFileSync(markers[0], 'utf8'))
    killProcessGroup(proc.pid!)
    expect(await withDeadline(closed, shutdownTimeout)).not.toBe('timed out')
    expect(await endsWithin(script, shutdownTimeout)).toBe(true)
  } finally {
    if (script != null) killProcess(script)
  }
})

function killProcess (pid: number): void {
  try {
    process.kill(pid, 'SIGKILL')
  } catch {
    // gone already
  }
}

async function waitForFile (file: string): Promise<void> {
  const deadline = Date.now() + shutdownTimeout
  while (!fs.existsSync(file)) {
    if (Date.now() > deadline) throw new Error(`${file} did not appear within ${shutdownTimeout}ms`)
    await new Promise<void>((resolve) => setTimeout(resolve, 50)) // eslint-disable-line no-await-in-loop
  }
}

async function withDeadline<T> (promise: Promise<T>, timeout: number): Promise<T | 'timed out'> {
  let timer: NodeJS.Timeout | undefined
  const deadline = new Promise<'timed out'>((resolve) => {
    timer = setTimeout(() => resolve('timed out'), timeout)
  })
  try {
    return await Promise.race([promise, deadline])
  } finally {
    clearTimeout(timer)
  }
}

/**
 * Runs the fixture's `script` in a session without a terminal, sends
 * `signal` to the runner once the script has started, and resolves with the
 * runner's exit code and whether the script had finished shutting down by
 * the time the runner exited. That moment is what matters: the script may
 * outlive the runner, and its output on the shared pipe would not tell.
 */
async function runWithoutTerminal (script: string, signal: NodeJS.Signals): Promise<{ shutDownBeforeExit: boolean, code: number | null }> {
  const proc = spawn(process.execPath, [runScript, script], { cwd: fixture, detached: true, stdio: ['ignore', 'pipe', 'inherit'] })
  const exited = new Promise<{ shutDownBeforeExit: boolean, code: number | null }>((resolve) => {
    proc.on('exit', (code) => {
      resolve({ shutDownBeforeExit: fs.existsSync(markers[1]), code })
    })
  })
  const closed = new Promise<void>((resolve) => {
    proc.on('close', () => {
      resolve()
    })
  })
  const killTimer = setTimeout(() => {
    killProcessGroup(proc.pid!)
  }, shutdownTimeout)
  let stdout = ''
  proc.stdout.setEncoding('utf8')
  const started = new Promise<void>((resolve) => {
    proc.stdout.on('data', (data: string) => {
      stdout += data
      if (stdout.includes('started')) resolve()
    })
  })
  try {
    await Promise.race([started, exited])
    proc.kill(signal)
    return await exited
  } finally {
    clearTimeout(killTimer)
    // A survivor would hold the output pipe open, so the group goes first
    // and the pipe closes on its own after that.
    killProcessGroup(proc.pid!)
    await closed
  }
}
