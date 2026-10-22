import path from 'node:path'

import { expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { resolveStoreLocationSettings } from '@pnpm/workspace.state'

const stateDir = path.resolve('/workspace')

test('relative and absolute spellings of a store location resolve to the same directory', () => {
  const store = path.join(stateDir, '.store', STORE_VERSION)
  for (const storeDir of ['.store', path.join(stateDir, '.store'), store]) {
    expect(resolveStoreLocationSettings({ storeDir }, stateDir).storeDir).toBe(store)
  }
  expect(resolveStoreLocationSettings({ virtualStoreDir: 'node_modules/.vs' }, stateDir).virtualStoreDir)
    .toBe(path.join(stateDir, 'node_modules', '.vs'))
})

test('a store location that is not configured stays unset', () => {
  expect(resolveStoreLocationSettings({}, stateDir)).toStrictEqual({ storeDir: undefined, virtualStoreDir: undefined })
})
