import chalk from 'chalk'
import { formatUnknownOptionsError } from '../src/formatError'

const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')

test('formatUnknownOptionsError()', async () => {
  expect(
    formatUnknownOptionsError(new Map([['foo', []]]))
  ).toBe(
    `${ERROR} ${chalk.red("Unknown option: 'foo'")}`
  )
  expect(
    formatUnknownOptionsError(new Map([['foo', ['foa', 'fob']]]))
  ).toBe(
    `${ERROR} ${chalk.red("Unknown option: 'foo'")}
Did you mean 'foa', or 'fob'? Use "--config.unknown=value" to force an unknown option.`
  )
  expect(
    formatUnknownOptionsError(new Map([['foo', []], ['bar', []]]))
  ).toBe(
    `${ERROR} ${chalk.red("Unknown options: 'foo', 'bar'")}`
  )
})
