import { remove } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import test = require('tape')

test('remove arg completions', async (t) => {
  prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })
  t.deepEqual(await remove.completion({}, []), [
    { name: 'is-negative' },
    { name: 'is-positive' },
  ])
  t.end()
})
