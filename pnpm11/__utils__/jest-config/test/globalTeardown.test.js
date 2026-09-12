import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'

import globalTeardown from '../with-registry/globalTeardown.js'
import { STORAGE_PREFIX } from '../with-registry/storagePrefix.js'

const created = []

afterEach(() => {
  delete global.killServer
  delete global.registryMockStorage
  // These tests are about a directory leak, so they must not leak one
  // themselves when an assertion fails before the code under test
  // removes it.
  while (created.length > 0) {
    rmSync(created.pop(), { recursive: true, force: true })
  }
})

function createStorage (prefix = STORAGE_PREFIX) {
  const storage = mkdtempSync(path.join(tmpdir(), prefix))
  created.push(storage)
  // Non-empty, so a recursive removal is actually exercised.
  writeFileSync(path.join(storage, 'packument.json'), '{}')
  return storage
}

test('the storage directory is removed', async () => {
  const storage = createStorage()
  global.registryMockStorage = storage

  await globalTeardown()

  expect(existsSync(storage)).toBe(false)
})

// Cleanup has to survive a failing shutdown: `killServer` rejects when
// `treeKill` fails or pnpr outlives its 10-second wait, and skipping the
// removal there is the leak this teardown exists to prevent.
test('the storage directory is removed even when the shutdown fails', async () => {
  const storage = createStorage()
  global.registryMockStorage = storage
  const shutdownError = new Error('Timed out waiting for pnpr to exit')
  global.killServer = () => Promise.reject(shutdownError)

  await expect(globalTeardown()).rejects.toBe(shutdownError)

  expect(existsSync(storage)).toBe(false)
})

test('a run that never recorded a storage path tears down cleanly', async () => {
  await expect(globalTeardown()).resolves.toBeUndefined()
})

// A recursive force-delete driven by a mutable global is worth a guard:
// only a directory shaped like the one `globalSetup` created is removed.
test('a storage path outside the setup naming scheme is refused', async () => {
  const storage = createStorage('some-unrelated-directory-')
  global.registryMockStorage = storage

  await expect(globalTeardown()).rejects.toThrow(/Refusing to remove/)

  expect(existsSync(storage)).toBe(true)
})
