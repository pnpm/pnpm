import { expect, test } from '@jest/globals'

import { optionTypesToCompletions } from '../../src/completion/optionTypesToCompletions.js'

test('optionTypesToCompletions', () => {
  expect(optionTypesToCompletions({
    number: Number,
    string: String,
    boolean: Boolean,
  })).toStrictEqual([
    { name: '--number' },
    { name: '--string' },
    { name: '--boolean' },
    { name: '--no-boolean' },
  ])
})
