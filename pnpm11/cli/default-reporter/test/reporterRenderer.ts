import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { initDefaultReporter } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import type * as logs from '@pnpm/core-loggers'
import {
  lockfileVerificationLogger,
  progressLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import type { StreamParser } from '@pnpm/logger'
import { createStreamParser } from '@pnpm/logger'

const ERASE_TO_END_OF_DISPLAY = '\x1b[0J'

const LOCKFILE_TEXT = 'Lockfile passes supply-chain policies'

async function yieldTick (): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, 0))
}

async function waitFor (
  writes: string[],
  predicate: (writes: readonly string[]) => boolean,
  timeoutMs = 5000
): Promise<void> {
  const start = Date.now()
  while (!predicate(writes)) {
    if (Date.now() - start > timeoutMs) {
      throw new Error(`waitFor timed out after ${timeoutMs}ms; ${writes.length} writes`)
    }
    await new Promise(resolve => setTimeout(resolve, 5)) // eslint-disable-line no-await-in-loop
  }
}

// Regression test for the duplication bug introduced by pnpm/pnpm#12351 and
// fixed by restoring `ansi-diff`. The reporter must write only the
// *differential* between frames — unchanged sticky blocks like the lockfile
// verdict must not be re-written on any subsequent progress tick.
test('differential renderer does not reprint unchanged sticky blocks', async () => {
  const writes: string[] = []
  const mockProcess = {
    stdout: {
      columns: 120,
      rows: 24,
      write: (chunk: string) => {
        writes.push(chunk)
        return true
      },
    },
    stderr: { write: () => true },
  }

  const cwd = '/home/jane/project'
  const streamParser = createStreamParser()
  const stop = initDefaultReporter({
    streamParser: streamParser as StreamParser<logs.Log>,
    reportingOptions: { throttleProgress: 0 },
    context: {
      argv: ['install'],
      config: { dir: cwd } as Config & ConfigContext,
      process: mockProcess as unknown as NodeJS.Process,
    },
  })

  await yieldTick()

  lockfileVerificationLogger.debug({
    status: 'cached',
    verifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
    lockfilePath: `${cwd}/pnpm-lock.yaml`,
  })
  stageLogger.debug({ prefix: cwd, stage: 'resolution_started' })
  progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'resolved' })

  await waitFor(writes, w => w.some(s => stripAnsi(s).includes(LOCKFILE_TEXT)))

  progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'fetched' })

  await waitFor(writes, w => w.length >= 2)
  stop()

  const firstWrite = writes[0]
  expect(stripAnsi(firstWrite)).toContain(LOCKFILE_TEXT)

  for (const write of writes.slice(1)) {
    expect(stripAnsi(write)).not.toContain(LOCKFILE_TEXT)
  }
})

// Each write must end with the erase-to-end-of-display sequence so that
// anything an external process (e.g. an SSH passphrase prompt) wrote below
// the rendered frame is cleared. This was the reason pnpm/pnpm#12351 replaced
// `ansi-diff` in the first place; the fix keeps the erase but wraps it
// around the differential output.
test('each write clears external output below the frame', async () => {
  const writes: string[] = []
  const mockProcess = {
    stdout: {
      columns: 120,
      rows: 24,
      write: (chunk: string) => {
        writes.push(chunk)
        return true
      },
    },
    stderr: { write: () => true },
  }

  const cwd = '/home/jane/project'
  const streamParser = createStreamParser()
  const stop = initDefaultReporter({
    streamParser: streamParser as StreamParser<logs.Log>,
    reportingOptions: { throttleProgress: 0 },
    context: {
      argv: ['install'],
      config: { dir: cwd } as Config & ConfigContext,
      process: mockProcess as unknown as NodeJS.Process,
    },
  })

  await yieldTick()

  statsLogger.debug({ added: 1, prefix: cwd })
  statsLogger.debug({ added: 2, prefix: cwd })
  await waitFor(writes, w => w.length >= 1)

  stop()

  expect(writes.length).toBeGreaterThanOrEqual(1)
  for (const write of writes) {
    expect(write.endsWith(ERASE_TO_END_OF_DISPLAY)).toBe(true)
  }
})
