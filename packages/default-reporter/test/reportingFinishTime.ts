import { finishTimeLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import { createStreamParser } from '@pnpm/logger'
import { take } from 'rxjs/operators'

test('does not print finish time for help command', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['help'],
    },
    streamParser: createStreamParser(),
  })

  finishTimeLogger.debug({
    startedAt: 1665279402859,
    finishedAt: 1665279413671,
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

test('prints finish time for install command', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  finishTimeLogger.debug({
    startedAt: 1665279402859,
    finishedAt: 1665279413671,
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe('Done in 10.8s')
    },
  })
})
