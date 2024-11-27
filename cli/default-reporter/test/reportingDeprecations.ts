import { type Config } from '@pnpm/config'
import {
  deprecationLogger,
  stageLogger,
} from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import { createStreamParser } from '@pnpm/logger'
import { map, take } from 'rxjs/operators'
import chalk from 'chalk'
import normalizeNewline from 'normalize-newline'
import { formatWarn } from '../src/reporterForClient/utils/formatWarn'

test('prints summary of deprecated subdependencies', (done) => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
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

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatWarn(`${chalk.red('2 deprecated subdependencies found:')} bar@2.0.0, qar@3.0.0`)}`)
    },
  })
})
