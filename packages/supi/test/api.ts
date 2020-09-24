import * as pnpm from 'supi'
import { testDefaults } from './utils'
import test = require('tape')

test('API', (t) => {
  t.equal(typeof pnpm.install, 'function', 'exports install()')
  t.equal(typeof pnpm.linkFromGlobal, 'function', 'exports linkFromGlobal()')
  t.equal(typeof pnpm.link, 'function', 'exports link()')
  t.equal(typeof pnpm.linkToGlobal, 'function', 'exports linkToGlobal()')
  t.end()
})

// TODO: some sort of this validation might need to exist
// maybe a new property should be introduced
// this seems illogical as even though all save types are false,
// the dependency will be saved
// eslint-disable-next-line @typescript-eslint/dot-notation
test.skip('install fails when all saving types are false', async (t: test.Test) => {
  try {
    await pnpm.install({}, await testDefaults({ save: false, saveDev: false, saveOptional: false }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, 'Cannot install with save/saveDev/saveOptional all being equal false')
    t.end()
  }
})

test('install fails on optional = true but production = false', async (t: test.Test) => {
  try {
    const opts = await testDefaults({
      include: {
        dependencies: false,
        devDependencies: false,
        optionalDependencies: true,
      },
    })
    await pnpm.install({}, opts)
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, 'Optional dependencies cannot be installed without production dependencies')
    t.end()
  }
})
