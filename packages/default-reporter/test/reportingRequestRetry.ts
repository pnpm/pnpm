import { requestRetryLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
import { take } from 'rxjs/operators'
import chalk = require('chalk')
import test = require('tape')

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')

test('print warning about request retry', (t) => {
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

  t.plan(1)

  output$.pipe(take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${WARN} GET https://foo.bar/qar error (undefined). Will retry in 12.5 seconds. 4 retries left.`)
    },
  })
})
