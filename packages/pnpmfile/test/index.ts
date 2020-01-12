import { requirePnpmfile } from '@pnpm/pnpmfile'
import path = require('path')
import test = require('tape')

test('ignoring a pnpmfile that exports undefined', (t) => {
  const pnpmfile = requirePnpmfile(path.join(__dirname, 'pnpmfiles/undefined.js'), __dirname)
  t.equal(typeof pnpmfile, 'undefined')
  t.end()
})
