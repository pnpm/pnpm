import { expect, test } from '@jest/globals'
import { remove } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'

test('remove arg completions', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })
  expect(await remove.completion({}, [])).toStrictEqual([
    { name: 'is-negative' },
    { name: 'is-positive' },
  ])
})
