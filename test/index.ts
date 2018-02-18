import getConfigs from '@pnpm/config'
import test = require('tape')

test('getConfigs()', t => {
  t.ok(getConfigs())
  t.end()
})
