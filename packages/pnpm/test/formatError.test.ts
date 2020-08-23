import { formatUnknownOptionsError } from '../src/formatError'
import chalk = require('chalk')
import test = require('tape')

const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')

test('formatUnknownOptionsError()', async (t) => {
  t.equal(
    formatUnknownOptionsError(new Map([['foo', []]])),
    `${ERROR} ${chalk.red("Unknown option: 'foo'")}`
  )
  t.equal(
    formatUnknownOptionsError(new Map([['foo', ['foa', 'fob']]])),
    `${ERROR} ${chalk.red("Unknown option: 'foo'")}
Did you mean 'foa', or 'fob'? Use "--config.unknown=value" to force an unknown option.`
  )
  t.equal(
    formatUnknownOptionsError(new Map([['foo', []], ['bar', []]])),
    `${ERROR} ${chalk.red("Unknown options: 'foo', 'bar'")}`
  )
  t.end()
})
