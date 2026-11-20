import { expect, test } from '@jest/globals'
import { run } from '@pnpm/exec.commands'
import { prepare } from '@pnpm/prepare'

test('run completion', async () => {
  prepare({
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
