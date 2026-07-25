import { existsSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'

import globalTeardown from '../with-registry/globalTeardown.js'

afterEach(() => {
  delete global.killServer
  delete global.registryMockStorage
})

function createStorage () {
  const storage = mkdtempSync(path.join(tmpdir(), 'pnpm-registry-mock-storage-test-'))
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

// The leak this teardown exists to prevent came back if a rejecting
// `killServer` skipped the removal.
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
