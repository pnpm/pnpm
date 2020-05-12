import test = require('tape')
import optionTypesToCompletions from '../src/optionTypesToCompletions'

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
    ]
  )
  t.end()
})
