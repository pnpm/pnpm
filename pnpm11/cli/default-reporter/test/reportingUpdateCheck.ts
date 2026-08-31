import { setTimeout } from 'node:timers/promises'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import { updateCheckLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'

import type { ReporterPnpmConfig } from '../src/ReporterPnpmConfig.js'

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

test('print update notification when pnpm was installed by another package manager', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as ReporterPnpmConfig,
      env: {},
      process: {
        platform: 'linux',
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

test('print update notification for Corepack if the latest version is greater than the current', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as ReporterPnpmConfig,
      env: {
        COREPACK_ROOT: '/usr/bin/corepack',
      },
      process: {
        platform: 'linux',
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

test('print update notification when PNPM_HOME manages the pnpm in use', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as ReporterPnpmConfig,
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

test('print update notification for Corepack on Windows', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as ReporterPnpmConfig,
      env: {
        COREPACK_ROOT: 'C:\\corepack',
      },
      process: {
        platform: 'win32',
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
