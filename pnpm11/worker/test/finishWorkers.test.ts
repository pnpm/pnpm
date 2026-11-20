import { spawn } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'

const libIndex = new URL('../lib/index.js', import.meta.url).href

// Regression test for https://github.com/pnpm/pnpm/issues/12297: a worker
// call that arrives after finishWorkers() (e.g. a tarball fetch delayed by a
// network retry, finishing after the CLI's final drain) must not leave a
// worker thread alive — a leaked idle worker keeps the process from exiting.
test('process exits after a worker call that arrives after finishWorkers()', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-worker-straggler-'))
  const storeDir = path.join(tmp, 'store')
  fs.mkdirSync(storeDir)
  const scenario = path.join(tmp, 'scenario.mjs')
  fs.writeFileSync(scenario, `
const { finishWorkers, readPkgFromCafs } = await import(${JSON.stringify(libIndex)})
const ctx = { storeDir: ${JSON.stringify(storeDir)}, verifyStoreIntegrity: false }
await readPkgFromCafs(ctx, ${JSON.stringify(path.join(storeDir, 'idx-1'))}) // normal install work
await finishWorkers() // the CLI's final drain
await readPkgFromCafs(ctx, ${JSON.stringify(path.join(storeDir, 'idx-2'))}) // late straggler task
`)
  const exitCode = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [scenario], { stdio: 'ignore' })
    const timer = setTimeout(() => {
      child.kill('SIGKILL')
      resolve('leaked worker kept the process alive')
    }, 8000)
    child.on('error', reject)
    child.on('exit', (code) => {
      clearTimeout(timer)
      resolve(code)
    })
  })
  expect(exitCode).toBe(0)
}, 15000)
