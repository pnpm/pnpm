import { optionTypesToCompletions } from './optionTypesToCompletions'

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
