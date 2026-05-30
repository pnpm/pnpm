import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'

import { formatUnknownOptionsError } from '../src/formatError.js'

test('formatUnknownOptionsError()', async () => {
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['foo', []]])))
  ).toBe(
    "[ERROR] Unknown option: 'foo'"
  )
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['foo', ['foa', 'fob']]])))
  ).toBe(
    `[ERROR] Unknown option: 'foo'
Did you mean 'foa', or 'fob'? Use "--config.unknown=value" to force an unknown option.`
  )
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['foo', []], ['bar', []]])))
  ).toBe(
    "[ERROR] Unknown options: 'foo', 'bar'"
  )
})
