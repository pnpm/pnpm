import getConfigs from '@pnpm/config'
import test = require('tape')

test('getConfigs()', async (t) => {
  t.ok(await getConfigs())
  t.end()
})
