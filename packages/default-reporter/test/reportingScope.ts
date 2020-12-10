import { Config } from '@pnpm/config'
import { toOutput$ } from '@pnpm/default-reporter'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import delay from 'delay'
import { take } from 'rxjs/operators'

const scopeLogger = logger<object>('scope')

test('does not print scope of non-recursive install in a workspace', async (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 1,
    workspacePrefix: '/home/src',
  })

  const subscription = output$.subscribe({
    complete: () => done(),
    error: done,
    next: () => {
      done('should not log anything')
    },
  })

  await delay(10)
  done()
  subscription.unsubscribe()
})

test('prints scope of recursive install in a workspace when not all packages are selected', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 2,
    total: 10,
    workspacePrefix: '/home/src',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe('Scope: 2 of 10 workspace projects')
    },
  })
})

test('prints scope of recursive install in a workspace when all packages are selected', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 10,
    total: 10,
    workspacePrefix: '/home/src',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe('Scope: all 10 workspace projects')
    },
  })
})

test('prints scope of recursive install not in a workspace when not all packages are selected', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 2,
    total: 10,
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe('Scope: 2 of 10 projects')
    },
  })
})

test('prints scope of recursive install not in a workspace when all packages are selected', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 10,
    total: 10,
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe('Scope: all 10 projects')
    },
  })
})
