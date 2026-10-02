import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, rmSync, rmdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath, URL } from 'node:url'

const runner = fileURLToPath(new URL('./run.mjs', import.meta.url))

function database (context) {
  const directory = mkdtempSync(join(tmpdir(), 'pnpm-wasm-sqlite-'))
  context.after(() => rmSync(directory, { recursive: true, force: true }))
  return join(directory, 'index.db')
}

function start (database, operation) {
  const child = spawn(process.execPath, [runner, database, operation])
  let output = ''
  child.stdout.on('data', data => { output += data })
  child.stderr.on('data', data => { output += data })
  const completed = once(child, 'close').then(([code, signal]) => ({ code, signal, output }))
  return { child, completed }
}

async function hold (database) {
  const running = start(database, 'hold')
  await new Promise((resolve, reject) => {
    running.child.stdout.on('data', data => {
      if (data.toString().includes('transaction-held')) resolve()
    })
    running.completed.then(result => reject(new Error(`Writer exited before holding its transaction: ${result.output}`)), reject)
  })
  return running
}

test('WASM SQLite persists a database readable by a later invocation', async context => {
  const file = database(context)
  const written = await start(file, 'write').completed
  assert.equal(written.code, 0, written.output)
  const read = await start(file, 'read').completed
  assert.equal(read.code, 0, read.output)
  assert.match(read.output, /threadsafe=1/)
  assert.match(read.output, /integrity_check=ok/)
  assert.match(read.output, /journal_mode=delete/)
  assert.match(read.output, /rows=1/)
})

test('WASM SQLite excludes a concurrent writer', async context => {
  const file = database(context)
  const holder = await hold(file)
  const contender = await start(file, 'write').completed
  assert.equal(contender.code, 1, contender.output)
  assert.match(contender.output, /database is locked/)
  const committed = await holder.completed
  assert.equal(committed.code, 0, committed.output)
  const read = await start(file, 'read').completed
  assert.match(read.output, /integrity_check=ok/)
  assert.match(read.output, /rows=1/)
})

test('a crashed writer requires confirmed-dead lock cleanup and rolls its transaction back', async context => {
  const file = database(context)
  assert.equal((await start(file, 'write').completed).code, 0)
  const holder = await hold(file)
  holder.child.kill('SIGKILL')
  const killed = await holder.completed
  assert.equal(killed.signal, 'SIGKILL')
  const blocked = await start(file, 'write').completed
  assert.equal(blocked.code, 1, blocked.output)
  assert.match(blocked.output, /database is locked/)
  rmdirSync(`${file}.lock`)
  const recovered = await start(file, 'read').completed
  assert.equal(recovered.code, 0, recovered.output)
  assert.match(recovered.output, /integrity_check=ok/)
  assert.match(recovered.output, /rows=1/)
})
