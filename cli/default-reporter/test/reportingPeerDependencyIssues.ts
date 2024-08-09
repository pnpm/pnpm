import { peerDependencyIssuesLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
  logger,
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
    issuesByProjects: {
      '.': {
        missing: {},
        bad: {
          a: [
            {
              parents: [
                {
                  name: 'b',
                  version: '1.0.0',
                },
              ],
              foundVersion: '2',
              resolvedFrom: [],
              optional: false,
              wantedRange: '3',
            },
          ],
        },
        conflicts: [],
        intersections: {},
      },
    },
  })

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toContain('.')
    },
  })
})

test('print peer dependency issues error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = Object.assign(new Error('some error'), {
    code: 'ERR_PNPM_PEER_DEP_ISSUES',
    issuesByProjects: {
      '.': {
        missing: {},
        bad: {
          a: [
            {
              foundVersion: '2',
              parents: [
                {
                  name: 'b',
                  version: '1.0.0',
                },
              ],
              optional: false,
              resolvedFrom: [],
              wantedRange: '3',
            },
          ],
        },
        conflicts: [],
        intersections: {},
      },
    },
  })
  logger.error(err, err)

  expect.assertions(1)

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toContain('.')
    },
  })
})
