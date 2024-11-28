import { LAYOUT_VERSION } from '@pnpm/constants'
import { checkCompatibility } from '../lib/install/checkCompatibility'

test('fail if the store directory changed', () => {
  expect(() => {
    checkCompatibility({
      layoutVersion: LAYOUT_VERSION,
      storeDir: '/store/v1',
    } as any, // eslint-disable-line
    {
      storeDir: '/store/v10',
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
    } as any, // eslint-disable-line
    {
      storeDir: '/store/v10',
      modulesDir: 'node_modules',
      virtualStoreDir: 'node_modules/.pnpm',
    })
  }).not.toThrow()
})
