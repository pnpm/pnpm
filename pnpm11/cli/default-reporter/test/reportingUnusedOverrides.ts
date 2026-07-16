import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import {
  stageLogger,
  unusedOverrideLogger,
} from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import normalizeNewline from 'normalize-newline'
import { firstValueFrom } from 'rxjs'
import { map, take } from 'rxjs/operators'

import { formatWarn } from '../src/reporterForClient/utils/formatWarn.js'

test('prints a single-line summary of unused overrides at resolution_done', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  unusedOverrideLogger.debug({
    prefix,
    selector: 'foo',
  })
  unusedOverrideLogger.debug({
    prefix,
    selector: 'parent>child',
  })
  unusedOverrideLogger.debug({
    prefix,
    selector: 'bar@1.0.0',
  })

  stageLogger.debug({
    prefix,
    stage: 'resolution_done',
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  expect(output).toBe(formatWarn('3 overrides matched no dependency: bar@1.0.0, foo, parent>child'))
})

test('uses singular form for a single unused override', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  unusedOverrideLogger.debug({
    prefix,
    selector: 'foo',
  })

  stageLogger.debug({
    prefix,
    stage: 'resolution_done',
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  expect(output).toBe(formatWarn('1 override matched no dependency: foo'))
})

test('does not print anything when no unused-override events were emitted', async () => {
  // A reporter-only contract: with no `pnpm:unused-override` events on
  // the stream, `resolution_done` must produce no warning frame. Whether
  // the underlying matcher ran and found everything used is an
  // install-layer concern covered by the integration test in
  // `installing/deps-installer/test/install/overrides.ts`.
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  stageLogger.debug({
    prefix,
    stage: 'resolution_done',
  })

  const NO_OUTPUT = Symbol('test should not log anything')
  const output = await Promise.race([
    firstValueFrom(output$.pipe(take(1), map(normalizeNewline))),
    new Promise<symbol>((resolve) => setTimeout(() => resolve(NO_OUTPUT), 50)),
  ])

  expect(output).toBe(NO_OUTPUT)
})

test('strips control characters from selectors before rendering', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  unusedOverrideLogger.debug({
    prefix,
    selector: 'foo\nbar\x1b[0m',
  })

  stageLogger.debug({
    prefix,
    stage: 'resolution_done',
  })

  expect.assertions(1)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  expect(output).toBe(formatWarn('1 override matched no dependency: foobar[0m'))
})
