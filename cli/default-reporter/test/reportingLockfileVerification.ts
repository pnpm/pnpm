import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import { lockfileVerificationLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom, take, toArray } from 'rxjs'

test('prints lockfile verification in-progress and completion messages', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  // Subscribe before emitting so we capture both the started and the
  // done frame in ansi-diff mode.
  const frames = firstValueFrom(output$.pipe(take(2), toArray()))

  lockfileVerificationLogger.debug({ status: 'started', entries: 234 })
  lockfileVerificationLogger.debug({
    status: 'done',
    entries: 234,
    elapsedMs: 1234,
  })

  const [started, done] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile (234 entries)...')
  expect(stripAnsi(done)).toBe('✓ Lockfile verified (234 entries in 1.2s)')
})

test('uses singular noun for one entry', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(2), toArray()))

  lockfileVerificationLogger.debug({ status: 'started', entries: 1 })
  lockfileVerificationLogger.debug({
    status: 'done',
    entries: 1,
    elapsedMs: 42,
  })

  const [started, done] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile (1 entry)...')
  expect(stripAnsi(done)).toBe('✓ Lockfile verified (1 entry in 42ms)')
})
