import getConfigs from '@pnpm/config'
import test = require('tape')

test('getConfigs()', async (t) => {
  const configs = await getConfigs({
    cliArgs: [],
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  t.ok(configs)
  t.equal(configs.fetchRetries, 2)
  t.equal(configs.fetchRetryFactor, 10)
  t.equal(configs.fetchRetryMintimeout, 10000)
  t.equal(configs.fetchRetryMaxtimeout, 60000)
  t.end()
})
