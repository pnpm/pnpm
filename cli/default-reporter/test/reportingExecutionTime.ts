import { setTimeout } from 'node:timers/promises'
import { executionTimeLogger } from '@pnpm/core-loggers'
import { packageManager } from '@pnpm/cli-meta'
import { toOutput$ } from '@pnpm/default-reporter'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'

const NO_OUTPUT = Symbol('test should not log anything')

test('does not print execution time for help command', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['help'],
    },
    streamParser: createStreamParser(),
  })

  executionTimeLogger.debug({
    startedAt: 1665279402859,
    endedAt: 1665279413671,
  })

  const output = await Promise.race([
    firstValueFrom(output$),
    setTimeout(10).then(() => NO_OUTPUT),
  ])

  expect(output).toEqual(NO_OUTPUT)
})

test('prints execution time for install command', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`Done in 10.8s using ${packageManager.name} v${packageManager.version}`)
})
