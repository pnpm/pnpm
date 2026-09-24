import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'

import { DirLock } from '../src/dirLock.js'

const OPTS = { waitMs: 0, abandonedMs: 60_000 }
const temporaryDirectories: string[] = []

afterEach(() => {
  for (const dir of temporaryDirectories.splice(0)) fs.rmSync(dir, { force: true, recursive: true })
})

function temporaryDirectory (): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dir-lock-'))
  temporaryDirectories.push(dir)
  return dir
}

test('a held lock is not acquired until it is released', async () => {
  const lockPath = path.join(temporaryDirectory(), 'held.lock')
  const holder = await DirLock.acquire(lockPath, OPTS)
  expect(holder).toBeDefined()

  expect(await DirLock.acquire(lockPath, OPTS)).toBeUndefined()

  await holder!.release()
  const next = await DirLock.acquire(lockPath, OPTS)
  expect(next).toBeDefined()
  await next!.release()
})

test('a lock whose holder process has ended is taken over at once', async () => {
  const lockPath = path.join(temporaryDirectory(), 'ended.lock')
  const { pid } = spawnSync(process.execPath, ['-e', ''])
  fs.mkdirSync(lockPath)
  fs.writeFileSync(path.join(lockPath, 'owner'), `${os.hostname()}:${pid}:0:ended`)

  const lock = await DirLock.acquire(lockPath, OPTS)
  expect(lock).toBeDefined()
  expect(await lock!.isOwner()).toBe(true)
  await lock!.release()
})
