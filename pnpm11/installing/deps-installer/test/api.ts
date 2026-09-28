import { expect, test } from '@jest/globals'
import * as pnpm from '@pnpm/installing.deps-installer'

import { testDefaults } from './utils/index.js'

test('API', () => {
  expect(typeof pnpm.install).toBe('function')
})

// TODO: some sort of this validation might need to exist
// maybe a new property should be introduced
// this seems illogical as even though all save types are false,
// the dependency will be saved
test.skip('install fails when all saving types are false', async () => {
  try {
    await pnpm.install({}, testDefaults({ save: false, saveDev: false, saveOptional: false }))
    throw new Error('installation should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toBe('Cannot install with save/saveDev/saveOptional all being equal false')
  }
})
