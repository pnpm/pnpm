import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import {
  deprecationLogger,
  stageLogger,
} from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import chalk from 'chalk'
import normalizeNewline from 'normalize-newline'
import { firstValueFrom } from 'rxjs'
import { map, take } from 'rxjs/operators'

import { formatWarn } from '../src/reporterForClient/utils/formatWarn.js'
import type { ReporterPnpmConfig } from '../src/ReporterPnpmConfig.js'

test('prints summary of deprecated subdependencies', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as ReporterPnpmConfig,
    },
    streamParser: createStreamParser(),
  })

  deprecationLogger.debug({
    depth: 1,
    pkgId: 'registry.npmjs.org/bar/2.0.0',
    pkgName: 'bar',
    pkgVersion: '2.0.0',
    prefix,
  })
  deprecationLogger.debug({
    depth: 2,
    pkgId: 'registry.npmjs.org/qar/3.0.0',
    pkgName: 'qar',
    pkgVersion: '3.0.0',
    prefix,
  })
  stageLogger.debug({
    prefix,
    stage: 'resolution_done',
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  expect(output).toBe(`${formatWarn(`${chalk.red('2 deprecated subdependencies found:')} bar@2.0.0, qar@3.0.0`)}`)
})

test('reports a deprecated direct dependency without the registry notice', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as ReporterPnpmConfig,
    },
    streamParser: createStreamParser(),
  })

  deprecationLogger.debug({
    depth: 0,
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    pkgName: 'foo',
    pkgVersion: '1.0.0',
    prefix,
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  expect(output).toBe(formatWarn(`${chalk.red('deprecated')} foo@1.0.0`))
})

test('strips control characters from a package name the manifest chose', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as ReporterPnpmConfig,
    },
    streamParser: createStreamParser(),
  })

  deprecationLogger.debug({
    depth: 0,
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    pkgName: 'foo\u001b[2K\rnot-really-deprecated',
    pkgVersion: '1.0.0',
    prefix,
  })

  expect.assertions(3)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  // The payload stays readable rather than being removed: without the
  // leading ESC the terminal prints it instead of acting on it.
  expect(output).not.toContain('\u001b')
  expect(output).not.toContain('\r')
  expect(output).toBe(formatWarn(`${chalk.red('deprecated')} foo[2Knot-really-deprecated@1.0.0`))
})
