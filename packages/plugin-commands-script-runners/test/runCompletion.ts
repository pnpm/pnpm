import { run } from '@pnpm/plugin-commands-script-runners'
import prepare from '@pnpm/prepare'

test('run completion', async () => {
  prepare(undefined, {
    scripts: {
      lint: 'eslint',
      test: 'node test.js',
    },
  })

  expect(
    await run.completion({}, [])
  ).toStrictEqual(
    [
      {
        name: 'lint',
      },
      {
        name: 'test',
      },
    ]
  )

  expect(await run.completion({}, ['test'])).toStrictEqual([])
})
