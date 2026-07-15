import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { initDefaultReporter } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import type * as logs from '@pnpm/core-loggers'
import {
  lockfileVerificationLogger,
  progressLogger,
  promptLogger,
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

  try {
    await yieldTick()

    lockfileVerificationLogger.debug({
      status: 'cached',
      verifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
      lockfilePath: `${cwd}/pnpm-lock.yaml`,
    })
    stageLogger.debug({ prefix: cwd, stage: 'resolution_started' })
    progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'resolved' })

    await waitFor(writes, w => w.some(s => stripAnsi(s).includes(LOCKFILE_TEXT)))

    const writesBeforeFetch = writes.length
    progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'fetched' })

    await waitFor(writes, w => w.length > writesBeforeFetch)

    // The sticky verdict must be written exactly once. Locate its first render
    // rather than assuming it lands in writes[0] (the reporter may emit an
    // initial frame before the verdict), then assert no later write reprints it.
    const firstStickyIndex = writes.findIndex(w => stripAnsi(w).includes(LOCKFILE_TEXT))
    expect(firstStickyIndex).toBeGreaterThanOrEqual(0)

    for (const write of writes.slice(firstStickyIndex + 1)) {
      expect(stripAnsi(write)).not.toContain(LOCKFILE_TEXT)
    }
  } finally {
    stop()
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

  try {
    await yieldTick()

    statsLogger.debug({ added: 1, prefix: cwd })
    statsLogger.debug({ added: 2, prefix: cwd })
    await waitFor(writes, w => w.length >= 1)

    expect(writes.length).toBeGreaterThanOrEqual(1)
    for (const write of writes) {
      expect(write.endsWith(ERASE_TO_END_OF_DISPLAY)).toBe(true)
    }
  } finally {
    stop()
  }
})

// Regression test for pnpm/pnpm#13019: a background progress tick that redrew
// in place while an interactive prompt (the strict minimumReleaseAge approval)
// was open moved the cursor into the prompt's lines and erased them, leaving
// the install hanging on an invisible question. The prompt brackets its
// lifetime with `pnpm:prompt` start/end events; the reporter must hold every
// frame redraw in between, then resume once the prompt releases the terminal.
test('holds frame redraws while an interactive prompt owns the terminal', async () => {
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

  const reportResolved = (id: string): void => {
    progressLogger.debug({ packageId: `registry.npmjs.org/${id}/1.0.0`, requester: cwd, status: 'resolved' })
  }

  try {
    await yieldTick()

    stageLogger.debug({ prefix: cwd, stage: 'resolution_started' })
    reportResolved('a')
    reportResolved('b')
    await waitFor(writes, w => w.length >= 1)
    const writesBeforePrompt = writes.length

    promptLogger.debug({ action: 'start' })
    // Background resolution keeps ticking while the prompt waits; each of these
    // would redraw a fresh "resolved N" frame if the reporter weren't paused.
    reportResolved('c')
    reportResolved('d')
    await new Promise(resolve => setTimeout(resolve, 200))

    expect(writes).toHaveLength(writesBeforePrompt)

    promptLogger.debug({ action: 'end' })
    reportResolved('e')
    reportResolved('f')
    await waitFor(writes, w => w.length > writesBeforePrompt)

    expect(writes.length).toBeGreaterThan(writesBeforePrompt)
  } finally {
    stop()
  }
})
