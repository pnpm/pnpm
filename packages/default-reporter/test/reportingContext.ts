import { contextLogger, packageImportMethodLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
import delay from 'delay'
import { take } from 'rxjs/operators'
import test = require('tape')

test('print context and import method info', (t) => {
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

  t.plan(1)

  output$.pipe(take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `\
Packages are hard linked from the content-addressable store to the virtual store.
  Content-addressable store is at: ~/.pnpm-store/v3
  Virtual store is at:             node_modules/.pnpm`)
    },
  })
})

test('do not print info if not fresh install', async (t) => {
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

  t.plan(1)

  const subscription = output$.subscribe({
    complete: () => t.end(),
    error: t.end,
    next: (msg) => {
      t.notOk(msg)
    },
  })

  await delay(10)
  t.ok('output$ has no event')
  subscription.unsubscribe()
})
