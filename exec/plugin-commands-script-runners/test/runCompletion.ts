import { prepare } from '@pnpm/prepare'
import { run } from '@pnpm/plugin-commands-script-runners'

test('run completion', async () => {
  prepare({
    scripts: {
      lint: 'eslint',
      test: 'node test.js',
    },
  })

  expect(await run.completion({}, [])).toStrictEqual([
    {
      name: 'lint',
    },
    {
      name: 'test',
    },
  ])

  expect(await run.completion({}, ['test'])).toStrictEqual([])
})
