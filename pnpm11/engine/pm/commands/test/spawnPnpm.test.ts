import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { spawnPnpm } from '../src/spawnPnpm.js'

const posixTest = process.platform === 'win32' ? test.skip : test

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
  await new Promise<void>((resolve) => {
    const timer = setInterval(() => {
      if (fs.existsSync(path.join(dir, 'started'))) {
        clearInterval(timer)
        resolve()
      }
    }, 20)
  })
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
