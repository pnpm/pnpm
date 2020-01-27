import { optionTypesToCompletions } from '@pnpm/cli-utils'
import test = require('tape')

test('optionTypesToCompletions()', t => {
  t.deepEqual(
    optionTypesToCompletions({
      bar: String,
      foo: Boolean,
    }), [
      {
        name: '--bar',
      },
      {
        name: '--foo',
      },
      {
        name: '--no-foo',
      },
    ],
  )
  t.end()
})
