import { take } from 'rxjs/operators'

import { createStreamParser } from '@pnpm/logger'
import { toOutput$ } from '@pnpm/default-reporter'
import { executionTimeLogger } from '@pnpm/core-loggers'

test('does not print execution time for help command', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['help'],
    },
    streamParser: createStreamParser(),
  })

  executionTimeLogger.debug({
    startedAt: 1_665_279_402_859,
    endedAt: 1_665_279_413_671,
  })

  const subscription = output$.subscribe({
    complete: () => done(),
    error: done,
    next: () => {
      done('should not log anything')
    },
  })

  setTimeout(() => {
    done()
    subscription.unsubscribe()
  }, 10)
})

test('prints execution time for install command', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  executionTimeLogger.debug({
    startedAt: 1665279402859,
    endedAt: 1665279413671,
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: (output) => {
      expect(output).toBe('Done in 10.8s')
    },
  })
})
