import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { hasControllingTerminal } from '@pnpm/exec.npm-lifecycle'
import { tempDir } from '@pnpm/prepare'

import { spawnPnpm } from '../src/spawnPnpm.js'

const posixTest = process.platform === 'win32' ? test.skip : test
// Only without a controlling terminal does the pnpm it runs get a process group of its own.
const noTerminalTest = process.platform === 'win32' || hasControllingTerminal() ? test.skip : test

const START_DEADLINE_MS = 30_000

// Resolves once the pnpm under test has written `file`. Rejects when it exits
// first or does not start in time, so a broken fixture fails the test instead
// of hanging it.
async function waitForStart (file: string, running: Promise<unknown>): Promise<void> {
  let exited = false
  running.then(() => {
    exited = true
  }, () => {
    exited = true
  })
  const deadline = Date.now() + START_DEADLINE_MS
  return new Promise<void>((resolve, reject) => {
    const timer = setInterval(() => {
      const failure = fs.existsSync(file)
        ? undefined
        : exited
          ? new Error(`the pnpm under test exited before it wrote ${file}`)
          : Date.now() > deadline ? new Error(`the pnpm under test did not write ${file} in time`) : null
      if (failure === null) return
      clearInterval(timer)
      if (failure) reject(failure)
      else resolve()
    }, 20)
  })
}

// A stand-in for the pnpm that pnpm switches to. It writes a file when it
// starts, and shuts down when it receives SIGTERM.
function writeFakePnpm (dir: string): string {
  const script = path.join(dir, 'fake-pnpm.cjs')
  fs.writeFileSync(script, `#!/usr/bin/env node
const fs = require('node:fs')
const dir = process.argv[2]
process.on('SIGTERM', () => {
  fs.writeFileSync(dir + '/got-sigterm', '')
  process.exit(7)
})
fs.writeFileSync(dir + '/started', '')
setInterval(() => {}, 1000)
`)
  fs.chmodSync(script, 0o755)
  return script
}

posixTest('spawnPnpm relays a SIGTERM sent to pnpm to the pnpm it runs (#9948)', async () => {
  const dir = tempDir()
  const fakePnpm = writeFakePnpm(dir)

  const running = spawnPnpm(fakePnpm, [dir])
  await waitForStart(path.join(dir, 'started'), running)
  process.emit('SIGTERM')

  expect(await running).toStrictEqual({ status: 7, signal: null })
  expect(fs.existsSync(path.join(dir, 'got-sigterm'))).toBe(true)
})

test('spawnPnpm reports the exit status of the pnpm it runs', async () => {
  const dir = tempDir()
  const script = path.join(dir, 'exit-3.cjs')
  fs.writeFileSync(script, '#!/usr/bin/env node\nprocess.exit(3)\n')
  fs.chmodSync(script, 0o755)

  expect(await spawnPnpm(script, [])).toStrictEqual({ status: 3, signal: null })
})

test('spawnPnpm rejects when the pnpm cannot be started', async () => {
  const dir = tempDir()

  await expect(spawnPnpm(path.join(dir, 'missing'), [])).rejects.toThrow()
})

noTerminalTest('spawnPnpm reaches the scripts of a pnpm that does not pass a signal on (#9948)', async () => {
  const dir = tempDir()
  // The script a pnpm started. It shuts down on SIGTERM.
  fs.writeFileSync(path.join(dir, 'script.cjs'), `const fs = require('node:fs')
const dir = process.argv[2]
process.on('SIGTERM', () => {
  fs.writeFileSync(dir + '/got-sigterm', '')
  process.exit(0)
})
fs.writeFileSync(dir + '/started', '')
setInterval(() => {}, 1000)
`)
  // A pnpm that ends on SIGTERM without telling the script it started.
  const selfishPnpm = path.join(dir, 'selfish-pnpm.cjs')
  fs.writeFileSync(selfishPnpm, `#!/usr/bin/env node
const { spawn } = require('node:child_process')
const dir = process.argv[2]
spawn(process.execPath, [dir + '/script.cjs', dir], { stdio: 'inherit' })
process.on('SIGTERM', () => process.exit(0))
setInterval(() => {}, 1000)
`)
  fs.chmodSync(selfishPnpm, 0o755)

  const running = spawnPnpm(selfishPnpm, [dir])
  await waitForStart(path.join(dir, 'started'), running)
  process.emit('SIGTERM')
  await running

  expect(fs.existsSync(path.join(dir, 'got-sigterm'))).toBe(true)
})
