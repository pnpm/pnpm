import { run } from '@pnpm/plugin-commands-script-runners'
import prepare from '@pnpm/prepare'
import test = require('tape')

test('run completion', async (t) => {
  prepare(t, {
    scripts: {
      lint: 'eslint',
      test: 'node test.js',
    },
  })

  t.deepEqual(
    await run.completion({}, []),
    [
      {
        name: 'lint',
      },
      {
        name: 'test',
      },
    ]
  )

  t.deepEqual(await run.completion({}, ['test']), [],
    "don't suggest script completions if script name already typed")

  t.end()
})
