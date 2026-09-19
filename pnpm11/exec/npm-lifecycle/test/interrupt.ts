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
  const proc = spawn(process.execPath, [runScript], { detached: true, stdio: ['ignore', 'pipe', 'inherit'] })
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
  proc.kill('SIGINT')
  const code = await exited
  clearTimeout(killTimer)
  expect(stdout).not.toMatch(/forced/)
  expect(stdout).toMatch(/shut down/)
  expect(code).toBe(0)
})
