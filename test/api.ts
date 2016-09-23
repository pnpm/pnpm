import test = require('tape')
import * as pnpm from '../src'

test('API', t => {
  t.equal(typeof pnpm.install, 'function', 'exports install()')
  t.equal(typeof pnpm.install, 'function', 'exports installPkgDeps()')
  t.equal(typeof pnpm.uninstall, 'function', 'exports uninstall()')
  t.equal(typeof pnpm.linkFromGlobal, 'function', 'exports linkFromGlobal()')
  t.equal(typeof pnpm.linkFromRelative, 'function', 'exports linkFromRelative()')
  t.equal(typeof pnpm.linkToGlobal, 'function', 'exports linkToGlobal()')
  t.equal(typeof pnpm.prune, 'function', 'exports prune()')
  t.end()
})
