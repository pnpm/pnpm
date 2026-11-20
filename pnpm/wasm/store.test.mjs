import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath, URL } from 'node:url'

const probe = fileURLToPath(new URL('../tasks/ecosystem-e2e/webcontainer/runtime-probe/store-probe.mjs', import.meta.url))

test('store leases exclude another process and recover SQLite after a killed transaction', { timeout: 15000 }, async context => {
  const child = spawn(process.execPath, [probe], { stdio: ['ignore', 'pipe', 'pipe'] })
  context.after(() => child.kill('SIGKILL'))
  let output = ''
  child.stdout.on('data', chunk => { output += chunk })
  child.stderr.on('data', chunk => { output += chunk })
  assert.deepEqual(await once(child, 'exit'), [0, null], output)
  assert.match(output, /pnpm-wasm-store-probe-ok/)
})
