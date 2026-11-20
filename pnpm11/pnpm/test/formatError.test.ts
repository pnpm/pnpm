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
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['long', ['-L']]]), new Map([['long', '-l']])))
  ).toBe(
    `[ERROR] Unknown option: '-l'
Did you mean '-L'? Use "--config.unknown=value" to force an unknown option.`
  )
  expect(
    stripAnsi(formatUnknownOptionsError(new Map([['long', []], ['bar', []]]), new Map([['long', '-l']])))
  ).toBe(
    "[ERROR] Unknown options: '-l', 'bar'"
  )
})
