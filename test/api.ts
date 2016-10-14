import test = require('tape')
import * as pnpm from '../src'

test('API', t => {
  t.equal(typeof pnpm.install, 'function', 'exports install()')
  t.equal(typeof pnpm.installPkgs, 'function', 'exports installPkgs()')
  t.equal(typeof pnpm.uninstall, 'function', 'exports uninstall()')
  t.equal(typeof pnpm.linkFromGlobal, 'function', 'exports linkFromGlobal()')
  t.equal(typeof pnpm.linkFromRelative, 'function', 'exports linkFromRelative()')
  t.equal(typeof pnpm.linkToGlobal, 'function', 'exports linkToGlobal()')
  t.equal(typeof pnpm.prune, 'function', 'exports prune()')
  t.equal(typeof pnpm.prunePkgs, 'function', 'exports prunePkgs()')
  t.equal(typeof pnpm.cleanCache, 'function', 'exports cleanCache()')
  t.end()
})
