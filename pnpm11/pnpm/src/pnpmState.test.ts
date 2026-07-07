import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { loadJsonFileSync } from 'load-json-file'
import { writeJsonFileSync } from 'write-json-file'

import { readPnpmState, writePnpmState } from './pnpmState.js'

test('a write merges over the other features\' keys instead of clobbering them', async () => {
  prepareEmpty()
  const stateDir = process.cwd()
  writeJsonFileSync('pnpm-state.json', {
    lastUpdateCheck: 'yesterday',
    pnpmExecCommands: { '/some/workspace': '["tool"]' },
  })

  await writePnpmState(stateDir, { lastUpdateCheck: 'today' })

  expect(loadJsonFileSync('pnpm-state.json')).toStrictEqual({
    lastUpdateCheck: 'today',
    pnpmExecCommands: { '/some/workspace': '["tool"]' },
  })
})

test('a write re-reads the file, so a concurrent update to another key is kept', async () => {
  prepareEmpty()
  const stateDir = process.cwd()
  const { state } = await readPnpmState(stateDir)
  expect(state).toBeUndefined()

  // Another process writes between this process's read and write.
  writeJsonFileSync('pnpm-state.json', { lastUpdateCheck: 'concurrent' })

  await writePnpmState(stateDir, { pnpmExecCommands: { '/w': '["tool"]' } })

  expect(loadJsonFileSync('pnpm-state.json')).toStrictEqual({
    lastUpdateCheck: 'concurrent',
    pnpmExecCommands: { '/w': '["tool"]' },
  })
})

test('an unparsable state file is writable (rewriting loses nothing valid)', async () => {
  prepareEmpty()
  const stateDir = process.cwd()
  fs.writeFileSync('pnpm-state.json', '{ not json')

  const { state, writable } = await readPnpmState(stateDir)
  expect(state).toBeUndefined()
  expect(writable).toBe(true)

  await writePnpmState(stateDir, { lastUpdateCheck: 'recovered' })
  expect(loadJsonFileSync('pnpm-state.json')).toStrictEqual({ lastUpdateCheck: 'recovered' })
})

test('an unreadable state file is not writable, so its keys cannot be clobbered', async () => {
  prepareEmpty()
  const stateDir = process.cwd()
  // A directory at the file's path makes every read fail with a non-ENOENT
  // error (EISDIR), standing in for a permissions failure portably.
  fs.mkdirSync('pnpm-state.json')

  const { state, writable } = await readPnpmState(stateDir)
  expect(state).toBeUndefined()
  expect(writable).toBe(false)

  await writePnpmState(stateDir, { lastUpdateCheck: 'must not land' })
  expect(fs.statSync(path.join(stateDir, 'pnpm-state.json')).isDirectory()).toBe(true)
})
