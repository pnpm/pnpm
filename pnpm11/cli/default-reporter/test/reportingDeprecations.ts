import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
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

test('prints summary of deprecated subdependencies', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  deprecationLogger.debug({
    deprecated: 'This package was deprecated because bla bla bla',
    depth: 1,
    pkgId: 'registry.npmjs.org/bar/2.0.0',
    pkgName: 'bar',
    pkgVersion: '2.0.0',
    prefix,
  })
  deprecationLogger.debug({
    deprecated: 'This package was deprecated because bla bla bla',
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
