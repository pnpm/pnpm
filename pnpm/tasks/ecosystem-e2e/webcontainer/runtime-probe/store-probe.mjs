import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { existsSync, mkdtempSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'
import console from 'node:console'
import { setTimeout } from 'node:timers/promises'
import { fileURLToPath, URL } from 'node:url'

const runner = process.env.PNPM_STORE_PROBE_RUNNER ?? fileURLToPath(new URL('../../../../wasm/run.mjs', import.meta.url))
const artifact = process.env.PNPM_STORE_PROBE_WASM ?? fileURLToPath(new URL('../../../../../target/wasm-runtime-probe/wasm32-wasip1-threads/debug/pnpm-wasm-runtime-probe.wasm', import.meta.url))

const cleanup = []
await runStoreProbe()

async function runStoreProbe () {
  try {
    await verifyStore()
  } finally {
    for (const clean of cleanup.reverse()) {
      // eslint-disable-next-line no-await-in-loop -- Reap children before deleting their shared database.
      await clean()
    }
  }
  console.log('pnpm-wasm-store-probe-ok')
}

async function verifyStore () {
  const directory = mkdtempSync(path.join(tmpdir(), 'pnpm-wasm-store-'))
  cleanup.push(() => rmSync(directory, { recursive: true, force: true }))
  function start (mode) {
    const child = spawn(process.execPath, [runner, artifact], {
      env: { ...process.env, PROBE_DIR: directory, PROBE_STORE: mode },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    const completion = once(child, 'exit')
    cleanup.push(async () => {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
      await completion
      child.unref()
      child.stdout.destroy()
      child.stderr.destroy()
    })
    let output = ''
    child.stdout.on('data', chunk => { output += chunk })
    child.stderr.on('data', chunk => { output += chunk })
    return { child, completion, output: () => output }
  }
  console.log('Starting SQLite lease holder')
  const writer = start('hold')
  for (let attempt = 0; !writer.output().includes('store-lease-held'); attempt++) {
    assert.ok(attempt < 3000 && writer.child.exitCode === null, writer.output())
    // eslint-disable-next-line no-await-in-loop -- Poll until the previous lock attempt completes.
    await setTimeout(10)
  }
  const lock = path.join(directory, 'store-probe/index.db.lock')
  assert.ok(existsSync(lock))
  const journal = statSync(path.join(directory, 'store-probe/index.db-journal'))
  assert.ok(journal.size > 0)
  assert.equal(journal.mode & 0o777, 0o600)
  console.log('SQLite transaction held; starting competing process')
  const reader = start('read')
  await setTimeout(200)
  assert.equal(reader.child.exitCode, null, reader.output())
  assert.ok(existsSync(lock), 'waiting reader must not reap a live SQLite lock')
  console.log('Terminating holder to exercise crash recovery')
  writer.child.kill('SIGKILL')
  await writer.completion
  console.log('Holder exited; waiting for SQLite recovery')
  assert.deepEqual(await reader.completion, [0, null], reader.output())
  assert.match(reader.output(), /Rust SQLite store persistence/)
  assert.equal(existsSync(lock), false)
}
