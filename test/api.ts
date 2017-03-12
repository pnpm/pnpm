import test = require('tape')
import * as pnpm from '../src'

test('API', t => {
  t.equal(typeof pnpm.install, 'function', 'exports install()')
  t.equal(typeof pnpm.installPkgs, 'function', 'exports installPkgs()')
  t.equal(typeof pnpm.uninstall, 'function', 'exports uninstall()')
  t.equal(typeof pnpm.linkFromGlobal, 'function', 'exports linkFromGlobal()')
  t.equal(typeof pnpm.link, 'function', 'exports link()')
  t.equal(typeof pnpm.linkToGlobal, 'function', 'exports linkToGlobal()')
  t.equal(typeof pnpm.prune, 'function', 'exports prune()')
  t.equal(typeof pnpm.prunePkgs, 'function', 'exports prunePkgs()')
  t.end()
})

test('install fails when all saving types are false', async t => {
  try {
    await pnpm.install({save: false, saveDev: false, saveOptional: false})
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, 'Cannot install with save/saveDev/saveOptional all being equal false')
    t.end()
  }
})
