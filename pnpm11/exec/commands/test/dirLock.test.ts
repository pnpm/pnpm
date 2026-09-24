import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'

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
  const { pid, status } = spawnSync(process.execPath, ['-e', ''])
  expect({ pid, status }).toMatchObject({ pid: expect.any(Number), status: 0 })
  fs.mkdirSync(lockPath)
  fs.writeFileSync(path.join(lockPath, 'owner'), `${os.hostname()}:${pid}:0:ended`)

  const lock = await DirLock.acquire(lockPath, OPTS)
  expect(lock).toBeDefined()
  expect(await lock!.isOwner()).toBe(true)
  await lock!.release()
})

test('a lock directory whose holder died before writing its owner file is taken over', async () => {
  const lockPath = path.join(temporaryDirectory(), 'ownerless.lock')
  fs.mkdirSync(lockPath)
  const longAgo = new Date(Date.now() - 60_000)
  fs.utimesSync(lockPath, longAgo, longAgo)

  const lock = await DirLock.acquire(lockPath, OPTS)
  expect(lock).toBeDefined()
  await lock!.release()
})

test('a live holder keeps its lock', async () => {
  const lockPath = path.join(temporaryDirectory(), 'live.lock')
  fs.mkdirSync(lockPath)
  fs.writeFileSync(path.join(lockPath, 'owner'), `${os.hostname()}:${process.ppid}:0:live`)

  expect(await DirLock.acquire(lockPath, OPTS)).toBeUndefined()
})

test('waiters taking over one ended holder\'s lock end up with one holder', async () => {
  const { pid } = spawnSync(process.execPath, ['-e', ''])
  for (let attempt = 0; attempt < 20; attempt++) {
    const lockPath = path.join(temporaryDirectory(), 'contended.lock')
    fs.mkdirSync(lockPath)
    fs.writeFileSync(path.join(lockPath, 'owner'), `${os.hostname()}:${pid}:0:ended`)

    // eslint-disable-next-line no-await-in-loop
    const locks = await Promise.all(Array.from({ length: 4 }, async () => DirLock.acquire(lockPath, OPTS)))
    expect(locks.filter(Boolean)).toHaveLength(1)
  }
})

test('a live holder on this host keeps its lock past the abandonment age', async () => {
  const lockPath = path.join(temporaryDirectory(), 'long.lock')
  fs.mkdirSync(lockPath)
  fs.writeFileSync(path.join(lockPath, 'owner'), `${os.hostname()}:${process.ppid}:0:live`)
  const longAgo = new Date(Date.now() - 60_000)
  fs.utimesSync(lockPath, longAgo, longAgo)

  expect(await DirLock.acquire(lockPath, { waitMs: 0, abandonedMs: 1_000 })).toBeUndefined()
})

test('a lock held on another host is taken over once it is older than the abandonment age', async () => {
  const lockPath = path.join(temporaryDirectory(), 'remote.lock')
  fs.mkdirSync(lockPath)
  fs.writeFileSync(path.join(lockPath, 'owner'), `not-${os.hostname()}:1:0:remote`)
  const longAgo = new Date(Date.now() - 60_000)
  fs.utimesSync(lockPath, longAgo, longAgo)

  const lock = await DirLock.acquire(lockPath, { waitMs: 0, abandonedMs: 1_000 })
  expect(lock).toBeDefined()
  await lock!.release()
})

test('a reaper lock left behind as a file does not block takeovers', async () => {
  const lockPath = path.join(temporaryDirectory(), 'reaped.lock')
  fs.mkdirSync(lockPath)
  const longAgo = new Date(Date.now() - 60_000)
  fs.utimesSync(lockPath, longAgo, longAgo)
  fs.writeFileSync(`${lockPath}.reap`, '')
  fs.utimesSync(`${lockPath}.reap`, longAgo, longAgo)

  const lock = await DirLock.acquire(lockPath, { waitMs: 1_000, abandonedMs: 60_000 })
  expect(lock).toBeDefined()
  await lock!.release()
})

test('a lock whose holder cannot be signaled is taken over once older than the abandonment age', async () => {
  const killSpy = jest.spyOn(process, 'kill').mockImplementation(() => {
    const err = new Error('operation not permitted')
    Object.assign(err, { code: 'EPERM' })
    throw err
  })
  try {
    const lockPath = path.join(temporaryDirectory(), 'eperm.lock')
    fs.mkdirSync(lockPath)
    fs.writeFileSync(path.join(lockPath, 'owner'), `${os.hostname()}:999999:0:eperm`)

    expect(await DirLock.acquire(lockPath, { waitMs: 0, abandonedMs: 1_000 })).toBeUndefined()

    const longAgo = new Date(Date.now() - 60_000)
    fs.utimesSync(lockPath, longAgo, longAgo)
    const lock = await DirLock.acquire(lockPath, { waitMs: 0, abandonedMs: 1_000 })
    expect(lock).toBeDefined()
    await lock!.release()
  } finally {
    killSpy.mockRestore()
  }
})
