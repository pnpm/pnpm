import { optionTypesToCompletions } from '../src/optionTypesToCompletions'

test('optionTypesToCompletions()', () => {
  expect(
    optionTypesToCompletions({
      bar: String,
      foo: Boolean,
    })
  ).toStrictEqual([
    {
      name: '--bar',
    },
    {
      name: '--foo',
    },
    {
      name: '--no-foo',
    },
  ])
})
