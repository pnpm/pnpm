import { requestRetryLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'
import { formatWarn } from '../src/reporterForClient/utils/formatWarn'

test('print warning about request retry', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(formatWarn('GET https://foo.bar/qar error (undefined). Will retry in 12.5 seconds. 4 retries left.'))
})
