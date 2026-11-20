import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { setTimeout } from 'node:timers/promises'
import { URL } from 'node:url'

import { createFileLocks } from './file-locks.mjs'

function fixture (context) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-wasm-lease-'))
  const file = path.join(directory, 'lock')
  fs.writeFileSync(file, '')
  context.after(() => fs.rmSync(directory, { recursive: true, force: true }))
  return file
}

async function acquire (locks, descriptor, exclusive) {
  const deadline = Date.now() + 5000
  for (;;) {
    const result = locks.tryLock(descriptor, exclusive)
    if (result === 0) return
    assert.equal(result, 6)
    assert.ok(Date.now() < deadline, 'file lease acquisition timed out')
    // eslint-disable-next-line no-await-in-loop -- Poll until the previous lock attempt completes.
    await setTimeout(5)
  }
}

test('shared leases overlap and exclude an exclusive holder', async context => {
  const file = fixture(context)
  const locks = createFileLocks()
  const descriptors = Array.from({ length: 3 }, () => fs.openSync(file, 'r+'))
  context.after(() => { locks.close(); descriptors.forEach(descriptor => fs.closeSync(descriptor)) })
  await acquire(locks, descriptors[0], false)
  await acquire(locks, descriptors[1], false)
  assert.equal(locks.tryLock(descriptors[2], true), 6)
  locks.release(descriptors[0])
  assert.equal(locks.tryLock(descriptors[2], true), 6)
  locks.release(descriptors[1])
  await acquire(locks, descriptors[2], true)
  assert.equal(locks.tryLock(descriptors[0], false), 6)
})

test('independent processes exclude each other and SIGKILL releases the lease', async context => {
  const file = fixture(context)
  const source = `
    import fs from 'node:fs';
    import {setTimeout} from 'node:timers/promises';
    import {createFileLocks} from ${JSON.stringify(new URL('./file-locks.mjs', import.meta.url).href)};
    const locks=createFileLocks(); const fd=fs.openSync(process.argv[1],'r+');
    // eslint-disable-next-line no-await-in-loop -- Poll until the previous lock attempt completes.
    while(locks.tryLock(fd,true)!==0) await setTimeout(5);
    process.stdout.write('held'); process.stdin.resume();
  `
  const holder = spawn(process.execPath, ['--input-type=module', '-e', source, file])
  context.after(() => holder.kill('SIGKILL'))
  const exited = once(holder, 'exit')
  await once(holder.stdout, 'data')
  const locks = createFileLocks()
  const descriptor = fs.openSync(file, 'r+')
  context.after(() => { locks.close(); fs.closeSync(descriptor) })
  assert.equal(locks.tryLock(descriptor, true), 6)
  await setTimeout(10)
  assert.equal(locks.tryLock(descriptor, true), 6)
  holder.kill('SIGKILL')
  await exited
  await acquire(locks, descriptor, true)
})

test('a child process can acquire an unrelated file while its parent holds a lease', async context => {
  const first = fixture(context)
  const second = fixture(context)
  const locks = createFileLocks()
  const descriptor = fs.openSync(first, 'r+')
  context.after(() => { locks.close(); fs.closeSync(descriptor) })
  await acquire(locks, descriptor, true)
  const source = `
    import fs from 'node:fs';
    import {setTimeout} from 'node:timers/promises';
    import {createFileLocks} from ${JSON.stringify(new URL('./file-locks.mjs', import.meta.url).href)};
    const locks=createFileLocks(); const fd=fs.openSync(process.argv[1],'r+');
    const deadline=Date.now()+2000;
    while(locks.tryLock(fd,true)!==0) {
      if(Date.now()>deadline) throw new Error('Unrelated lock was blocked');
      await setTimeout(5);
    }
    locks.close(); fs.closeSync(fd);
  `
  const child = spawn(process.execPath, ['--input-type=module', '-e', source, second])
  context.after(() => child.kill('SIGKILL'))
  assert.deepEqual(await once(child, 'exit'), [0, null])
})

test('a failed exclusive upgrade retains its shared lease', async context => {
  const file = fixture(context)
  const locks = createFileLocks()
  const descriptors = Array.from({ length: 3 }, () => fs.openSync(file, 'r+'))
  context.after(() => { locks.close(); descriptors.forEach(descriptor => fs.closeSync(descriptor)) })
  await acquire(locks, descriptors[0], false)
  await acquire(locks, descriptors[1], false)
  assert.equal(locks.tryLock(descriptors[0], true), 6)
  await setTimeout(25)
  assert.equal(locks.tryLock(descriptors[0], false), 0)
  locks.release(descriptors[1])
  assert.equal(locks.tryLock(descriptors[2], true), 6)
  await acquire(locks, descriptors[0], true)
  assert.equal(locks.tryLock(descriptors[1], false), 6)
})

test('confirmed process death releases a lease despite a stale responsive listener', async context => {
  const file = fixture(context)
  const locks = createFileLocks()
  const descriptors = [fs.openSync(file, 'r+'), fs.openSync(file, 'r+')]
  context.after(() => { locks.close(); descriptors.forEach(descriptor => fs.closeSync(descriptor)) })
  await acquire(locks, descriptors[0], true)
  const kill = process.kill.bind(process)
  context.mock.method(process, 'kill', (pid, signal) => {
    if (pid === process.pid && signal === 0) throw Object.assign(new Error('No such process'), { code: 'ESRCH' })
    return kill(pid, signal)
  })
  await acquire(locks, descriptors[1], true)
})
