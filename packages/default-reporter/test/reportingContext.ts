import { contextLogger, packageImportMethodLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
import delay from 'delay'
import { take } from 'rxjs/operators'

test('print context and import method info', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  contextLogger.debug({
    currentLockfileExists: false,
    storeDir: '~/.pnpm-store/v3',
    virtualStoreDir: 'node_modules/.pnpm',
  })
  packageImportMethodLogger.debug({
    method: 'hardlink',
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`\
Packages are hard linked from the content-addressable store to the virtual store.
  Content-addressable store is at: ~/.pnpm-store/v3
  Virtual store is at:             node_modules/.pnpm`)
    },
  })
})

test('do not print info if not fresh install', async (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  contextLogger.debug({
    currentLockfileExists: true,
    storeDir: '~/.pnpm-store/v3',
    virtualStoreDir: 'node_modules/.pnpm',
  })
  packageImportMethodLogger.debug({
    method: 'hardlink',
  })

  const subscription = output$.subscribe({
    complete: () => done(),
    error: done,
    next: (msg) => {
      expect(msg).toBeFalsy()
    },
  })

  await delay(10)
  done()
  subscription.unsubscribe()
})
