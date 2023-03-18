import { type Config } from '@pnpm/config'
import { updateCheckLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import { createStreamParser } from '@pnpm/logger'
import { take } from 'rxjs/operators'
import stripAnsi from 'strip-ansi'

test('does not print update if latest is less than current', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      env: {},
    },
    streamParser: createStreamParser(),
  })

  updateCheckLogger.debug({
    currentVersion: '10.0.0',
    latestVersion: '9.0.0',
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

test('print update notification if the latest version is greater than the current', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
      env: {},
    },
    streamParser: createStreamParser(),
  })

  updateCheckLogger.debug({
    currentVersion: '10.0.0',
    latestVersion: '11.0.0',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(stripAnsi(output)).toMatchSnapshot()
    },
  })
})

test('print update notification for Corepack if the latest version is greater than the current', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
      env: {
        COREPACK_ROOT: '/usr/bin/corepack',
      },
    },
    streamParser: createStreamParser(),
  })

  updateCheckLogger.debug({
    currentVersion: '10.0.0',
    latestVersion: '11.0.0',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(stripAnsi(output)).toMatchSnapshot()
    },
  })
})

test('print update notification that suggests to use the standalone scripts for the upgrade', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
      env: {
        PNPM_HOME: '/home/user/.local/share/pnpm',
      },
      process: {
        pkg: true,
      } as any, // eslint-disable-line
    },
    streamParser: createStreamParser(),
  })

  updateCheckLogger.debug({
    currentVersion: '10.0.0',
    latestVersion: '11.0.0',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(stripAnsi(output)).toMatchSnapshot()
    },
  })
})
