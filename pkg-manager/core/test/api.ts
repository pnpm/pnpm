import * as pnpm from '@pnpm/core'
import { testDefaults } from './utils'

test('API', () => {
  expect(typeof pnpm.install).toBe('function')
  expect(typeof pnpm.link).toBe('function')
})

// TODO: some sort of this validation might need to exist
// maybe a new property should be introduced
// this seems illogical as even though all save types are false,
// the dependency will be saved
test.skip('install fails when all saving types are false', async () => {
  try {
    await pnpm.install({}, await testDefaults({ save: false, saveDev: false, saveOptional: false }))
    throw new Error('installation should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toBe('Cannot install with save/saveDev/saveOptional all being equal false')
  }
})

test('install fails on optional = true but production = false', async () => {
  try {
    const opts = await testDefaults({
      include: {
        dependencies: false,
        devDependencies: false,
        optionalDependencies: true,
      },
    })
    await pnpm.install({}, opts)
    throw new Error('installation should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toBe('Optional dependencies cannot be installed without production dependencies')
  }
})
