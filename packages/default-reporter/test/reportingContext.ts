import { contextLogger, packageImportMethodLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
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

  output$.take(1).subscribe({
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
