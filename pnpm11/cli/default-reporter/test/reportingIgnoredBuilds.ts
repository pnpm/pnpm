import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import { ignoredScriptsLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'

import { captureOutput } from './utils/captureOutput.js'

test('prints ignored build scripts in a box', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  ignoredScriptsLogger.debug({ packageNames: ['esbuild'] })

  const output = stripAnsi(await firstValueFrom(output$))
  expect(output).toContain('╭ Warning')
  expect(output).toContain('Ignored build scripts: esbuild.')
})

test('prints ignored build scripts as plain lines when append-only is used', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly: true },
    streamParser: createStreamParser(),
  })

  ignoredScriptsLogger.debug({ packageNames: ['esbuild', 'core-js'] })

  expect(stripAnsi(await firstValueFrom(output$))).toBe(`Ignored build scripts: core-js, esbuild.
Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.`)
})

test.each(['warn', 'error'] as const)('reports ignored build scripts only at warning tiers with loglevel=%s', async (logLevel) => {
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly: true, logLevel },
  }, () => {
    ignoredScriptsLogger.debug({ packageNames: ['esbuild'] })
  })
  expect(output).toEqual(logLevel === 'warn' ? [
    'Ignored build scripts: esbuild.\nRun "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.',
  ] : [])
})

test('does not duplicate the strict dependency-build error with a warning', async () => {
  const output = await captureOutput({
    context: { argv: ['install'], config: { strictDepBuilds: true } },
    reportingOptions: { appendOnly: true, logLevel: 'warn' },
  }, () => {
    ignoredScriptsLogger.debug({ packageNames: ['esbuild'] })
  })
  expect(output).toEqual([])
})
