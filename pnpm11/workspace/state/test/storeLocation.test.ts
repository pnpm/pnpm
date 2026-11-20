import path from 'node:path'

import { expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { resolveStoreLocationSettings } from '@pnpm/workspace.state'

const stateDir = path.resolve('/workspace')
const dirs = { stateDir, storeBaseDir: stateDir }

test('relative and absolute spellings of a store location resolve to the same directory', () => {
  const store = path.join(stateDir, '.store', STORE_VERSION)
  for (const storeDir of ['.store', path.join(stateDir, '.store'), store]) {
    expect(resolveStoreLocationSettings({ storeDir }, dirs).storeDir).toBe(store)
  }
  expect(resolveStoreLocationSettings({ virtualStoreDir: 'node_modules/.vs' }, dirs).virtualStoreDir)
    .toBe(path.join(stateDir, 'node_modules', '.vs'))
})

test('storeDir resolves against the store base and virtualStoreDir against the state directory', () => {
  const projectDir = path.join(stateDir, 'packages', 'project')
  expect(resolveStoreLocationSettings({ storeDir: '.store', virtualStoreDir: '.vs' }, { stateDir: projectDir, storeBaseDir: stateDir })).toStrictEqual({
    storeDir: path.join(stateDir, '.store', STORE_VERSION),
    virtualStoreDir: path.join(projectDir, '.vs'),
  })
})

test('a store location that is not configured stays unset', () => {
  expect(resolveStoreLocationSettings({}, dirs)).toStrictEqual({ storeDir: undefined, virtualStoreDir: undefined })
})
