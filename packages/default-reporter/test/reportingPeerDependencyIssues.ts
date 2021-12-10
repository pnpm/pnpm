import { peerDependencyIssuesLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import { take } from 'rxjs/operators'

test('print peer dependency issues warning', (done) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  peerDependencyIssuesLogger.debug({
    missing: {},
    bad: {
      a: [
        {
          location: {
            parents: [
              {
                name: 'b',
                version: '1.0.0',
              },
            ],
            projectPath: '',
          },
          foundVersion: '2',
          wantedRange: '3',
        },
      ],
    },
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toContain('<ROOT>')
    },
  })
})

test('print peer dependency issues error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  err['code'] = 'ERR_PNPM_PEER_DEP_ISSUES'
  err['issues'] = {
    missing: {},
    bad: {
      a: [
        {
          location: {
            parents: [
              {
                name: 'b',
                version: '1.0.0',
              },
            ],
            projectPath: '',
          },
          wantedRange: '3',
        },
      ],
    },
  }
  logger.error(err, err)

  expect.assertions(1)

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toContain('<ROOT>')
    },
  })
})
