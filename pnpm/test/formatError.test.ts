import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { formatUnknownOptionsError } from '../src/formatError.js'

test('formatUnknownOptionsError()', async () => {
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['foo', []]])))
  ).toBe(
    "\u2009ERROR\u2009 Unknown option: 'foo'"
  )
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['foo', ['foa', 'fob']]])))
  ).toBe(
    `\u2009ERROR\u2009 Unknown option: 'foo'
Did you mean 'foa', or 'fob'? Use "--config.unknown=value" to force an unknown option.`
  )
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['foo', []], ['bar', []]])))
  ).toBe(
    "\u2009ERROR\u2009 Unknown options: 'foo', 'bar'"
  )
})
