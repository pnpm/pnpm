import { expect, test } from '@jest/globals'

import { createHelp } from '../src/cmd/help.js'

test('print an error when help not found', () => {
  expect(
    (createHelp({}).handler({}, ['foo']) as string).split('\n')[1]
  ).toBe('No results for "foo"')
})

test('print an error when help is requested for an Object.prototype property name', () => {
  expect(
    (createHelp({}).handler({}, ['constructor']) as string).split('\n')[1]
  ).toBe('No results for "constructor"')
})
