import test = require('tape')
import * as pnpm from 'supi'

test('API', t => {
  t.equal(typeof pnpm.install, 'function', 'exports install()')
  t.equal(typeof pnpm.installPkgs, 'function', 'exports installPkgs()')
  t.equal(typeof pnpm.uninstall, 'function', 'exports uninstall()')
  t.equal(typeof pnpm.linkFromGlobal, 'function', 'exports linkFromGlobal()')
  t.equal(typeof pnpm.link, 'function', 'exports link()')
  t.equal(typeof pnpm.linkToGlobal, 'function', 'exports linkToGlobal()')
  t.equal(typeof pnpm.prune, 'function', 'exports prune()')
  t.end()
})

// TODO: some sort of this validation might need to exist
// maybe a new property should be introduced
// this seems illogical as even though all save types are false,
// the dependency will be saved
test.skip('install fails when all saving types are false', async (t: test.Test) => {
  try {
    await pnpm.install({save: false, saveDev: false, saveOptional: false})
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, 'Cannot install with save/saveDev/saveOptional all being equal false')
    t.end()
  }
})

test('install fails on optional = true but production = false', async (t: test.Test) => {
  try {
    await pnpm.install({optional: true, production: false})
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, 'Optional dependencies cannot be installed without production dependencies')
    t.end()
  }
})
