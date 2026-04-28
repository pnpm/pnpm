import { setTimeout } from 'node:timers/promises'

import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { scopeLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'

const NO_OUTPUT = Symbol('test should not log anything')

test('does not print scope of non-recursive install in a workspace', async () => {
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

  const output = await Promise.race([
    firstValueFrom(output$),
    setTimeout(10).then(() => NO_OUTPUT),
  ])

  expect(output).toEqual(NO_OUTPUT)
})

test('prints scope of recursive install in a workspace when not all packages are selected', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 2,
    total: 10,
    workspacePrefix: '/home/src',
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$)
  expect(output).toBe('Scope: 2 of 10 workspace projects')
})

test('prints scope of recursive install in a workspace when all packages are selected', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 10,
    total: 10,
    workspacePrefix: '/home/src',
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$)
  expect(output).toBe('Scope: all 10 workspace projects')
})

test('prints scope of recursive install not in a workspace when not all packages are selected', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 2,
    total: 10,
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$)
  expect(output).toBe('Scope: 2 of 10 projects')
})

test('prints scope of recursive install not in a workspace when all packages are selected', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 10,
    total: 10,
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$)
  expect(output).toBe('Scope: all 10 projects')
})
