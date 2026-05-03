import { expect, test } from '@jest/globals'
import { LAYOUT_VERSION } from '@pnpm/constants'

import { checkCompatibility } from '../lib/install/checkCompatibility/index.js'

test('fail if the store directory changed', () => {
  expect(() => {
    checkCompatibility({
      layoutVersion: LAYOUT_VERSION,
      storeDir: '/store/v1',
    } as Parameters<typeof checkCompatibility>[0],
    {
      storeDir: '/store/v11',
      modulesDir: 'node_modules',
      virtualStoreDir: 'node_modules/.pnpm',
    })
  }).toThrow('Unexpected store location')
})

test('do not fail if the store directory is of version 3', () => {
  expect(() => {
    checkCompatibility({
      layoutVersion: LAYOUT_VERSION,
      storeDir: '/store/v3',
    } as Parameters<typeof checkCompatibility>[0],
    {
      storeDir: '/store/v11',
      modulesDir: 'node_modules',
      virtualStoreDir: 'node_modules/.pnpm',
    })
  }).not.toThrow()
})
