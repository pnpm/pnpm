import createHelp from '../src/cmd/help'

test('print an error when help not found', () => {
  expect(
    createHelp({})({}, ['foo']).split('\n')[1]
  ).toBe('No results for "foo"')
})
