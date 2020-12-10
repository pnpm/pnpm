import { requestRetryLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
import { take } from 'rxjs/operators'
import chalk = require('chalk')

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')

test('print warning about request retry', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  requestRetryLogger.debug({
    attempt: 2,
    error: new Error(),
    maxRetries: 5,
    method: 'GET',
    timeout: 12500,
    url: 'https://foo.bar/qar',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${WARN} GET https://foo.bar/qar error (undefined). Will retry in 12.5 seconds. 4 retries left.`)
    },
  })
})
