import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare, tempDir } from '@pnpm/prepare'

import { spawnPnpm } from './utils/index.js'

// pnpm/tasks#133: when the reader of stdout is gone, pnpm drops the remaining
// output but finishes the command and exits with its own code.

test('pnpm clean --lockfile removes the lockfile when stdout is a closed pipe', async () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')
  fs.writeFileSync('pnpm-lock.yaml', 'lockfileVersion: 9')
  fs.mkdirSync('node_modules/.pnpm', { recursive: true })

  const { status } = await runWithClosedStdout(['clean', '--lockfile'])

  expect(status).toBe(0)
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(false)
})

test('a failing script keeps its exit code when stdout is a closed pipe', async () => {
  prepare({
    scripts: {
      fail: 'node -e "process.exit(2)"',
    },
  })

  const { status } = await runWithClosedStdout(['run', 'fail'])

  expect(status).toBe(2)
})

async function runWithClosedStdout (args: string[]): Promise<{ status: number | null }> {
  const proc = spawnPnpm(args)
  proc.stdout!.destroy()
  proc.stderr!.resume()
  const status = await new Promise<number | null>((resolve, reject) => {
    proc.on('error', reject)
    proc.on('close', resolve)
  })
  return { status }
}
