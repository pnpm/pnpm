import getConfigs from '@pnpm/config'
import test = require('tape')

test('getConfigs()', async (t) => {
  t.ok(await getConfigs({
    cliArgs: [],
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  }))
  t.end()
})
