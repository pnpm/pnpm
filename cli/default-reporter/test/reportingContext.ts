import { setTimeout } from 'node:timers/promises'
import { contextLogger, packageImportMethodLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
} from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'

const NO_OUTPUT = Symbol('test should not log anything')

test('print context and import method info', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`\
Packages are hard linked from the content-addressable store to the virtual store.
  Content-addressable store is at: ~/.pnpm-store/v3
  Virtual store is at:             node_modules/.pnpm`)
})

test('do not print info if not fresh install', async () => {
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

  const output = await Promise.race([
    firstValueFrom(output$),
    setTimeout(10).then(() => NO_OUTPUT),
  ])

  expect(output).toEqual(NO_OUTPUT)
})

test('do not print info if dlx is the executed command', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['dlx'],
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

  const output = await Promise.race([
    firstValueFrom(output$),
    setTimeout(10).then(() => NO_OUTPUT),
  ])

  expect(output).toEqual(NO_OUTPUT)
})
