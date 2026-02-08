import { createHelp } from '../src/cmd/help.js'

test('print an error when help not found', () => {
  expect(
    (createHelp({}, {}).handler({}, ['foo']) as string).split('\n')[1]
  ).toBe('No results for "foo"')
})
