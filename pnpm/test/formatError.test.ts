import chalk from 'chalk'
import { stripVTControlCharacters } from 'util'
import { formatUnknownOptionsError } from '../src/formatError.js'

const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')

test('formatUnknownOptionsError()', async () => {
  expect(
    stripVTControlCharacters(formatUnknownOptionsError(new Map([['foo', []]])))
  ).toBe(
    stripVTControlCharacters(`${ERROR} ${chalk.red("Unknown option: 'foo'")}`)
  )
  expect(
    stripVTControlCharacters(formatUnknownOptionsError(new Map([['foo', ['foa', 'fob']]])))
  ).toBe(
    stripVTControlCharacters(`${ERROR} ${chalk.red("Unknown option: 'foo'")}
Did you mean 'foa', or 'fob'? Use "--config.unknown=value" to force an unknown option.`)
  )
  expect(
    stripVTControlCharacters(formatUnknownOptionsError(new Map([['foo', []], ['bar', []]])))
  ).toBe(
    stripVTControlCharacters(`${ERROR} ${chalk.red("Unknown options: 'foo', 'bar'")}`)
  )
})
