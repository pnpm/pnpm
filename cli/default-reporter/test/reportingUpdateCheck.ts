import { setTimeout } from 'node:timers/promises'
import { type Config } from '@pnpm/config'
import { updateCheckLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'
import { stripVTControlCharacters as stripAnsi } from 'util'

const NO_OUTPUT = Symbol('test should not log anything')

test('does not print update if latest is less than current', async () => {
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

  const output = await Promise.race([
    firstValueFrom(output$),
    setTimeout(10).then(() => NO_OUTPUT),
  ])

  expect(output).toEqual(NO_OUTPUT)
})

test('print update notification if the latest version is greater than the current', async () => {
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

  const output = await firstValueFrom(output$)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('print update notification for Corepack if the latest version is greater than the current', async () => {
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

  const output = await firstValueFrom(output$)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('print update notification that suggests to use the standalone scripts for the upgrade', async () => {
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

  const output = await firstValueFrom(output$)
  expect(stripAnsi(output)).toMatchSnapshot()
})
