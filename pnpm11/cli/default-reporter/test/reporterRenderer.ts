import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { initDefaultReporter } from '@pnpm/cli.default-reporter'
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

import type { ReporterPnpmConfig } from '../src/ReporterPnpmConfig.js'

const ERASE_TO_END_OF_DISPLAY = '\x1b[0J'

const LOCKFILE_TEXT = 'Lockfile passes supply-chain policies'

async function yieldTick (): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, 0))
}

/** The cursor-up distances (`\x1b[<n>A`) in `output`. */
function cursorUps (output: string): number[] {
  const distances: number[] = []
  for (const part of output.split('\x1b[').slice(1)) {
    let end = 0
    while (part[end] >= '0' && part[end] <= '9') end++
    if (end > 0 && part[end] === 'A') distances.push(Number(part.slice(0, end)))
  }
  return distances
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
      config: { dir: cwd } as ReporterPnpmConfig,
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
      config: { dir: cwd } as ReporterPnpmConfig,
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
      config: { dir: cwd } as ReporterPnpmConfig,
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

// Regression test for pnpm/pnpm#14270: `pnpm update -g` installs each global
// package group in turn, so the frame keeps growing by one progress line per
// group. `ansi-diff` redraws a line by moving the cursor up from the end of the
// frame; once the frame is taller than the terminal, the lines it has to reach
// have scrolled away and the move stops at the top of the screen — landing on,
// and overwriting, whatever is displayed there instead.
test('never redraws above the top of the terminal', async () => {
  const rows = 6
  const writes: string[] = []
  const mockProcess = {
    stdout: {
      columns: 120,
      rows,
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
      argv: ['update'],
      config: { dir: cwd } as ReporterPnpmConfig,
      process: mockProcess as unknown as NodeJS.Process,
    },
  })

  const groupDir = (group: number): string => `${cwd}/global/install-${group}`
  const reportResolved = (group: number, id: string): void => {
    progressLogger.debug({
      packageId: `registry.npmjs.org/${id}/1.0.0`,
      requester: groupDir(group),
      status: 'resolved',
    })
  }

  try {
    await yieldTick()

    // One progress line per group, until the frame is taller than the terminal.
    const groupCount = rows * 3
    for (let group = 0; group < groupCount; group++) {
      stageLogger.debug({ prefix: groupDir(group), stage: 'resolution_started' })
      reportResolved(group, 'foo')
    }
    await waitFor(writes, w => w.some(s => stripAnsi(s).includes(`install-${groupCount - 1}`)))

    // The first group's line sits at the top of the frame, which by now has
    // scrolled off the screen. Redrawing it is what walked the cursor too far.
    const writesBeforeRedraw = writes.length
    reportResolved(0, 'bar')
    await waitFor(writes, w => w.length > writesBeforeRedraw)

    const ups = writes.flatMap(cursorUps)
    expect(ups.length).toBeGreaterThan(0)
    expect(Math.max(...ups)).toBeLessThan(rows)
  } finally {
    stop()
  }
})

// A resize reflows the frame already on screen, so nothing `ansi-diff` tracked
// against the old width still describes where anything is. The frame has to be
// drawn afresh below rather than diffed against a layout that no longer holds.
test('a resize starts a fresh frame', async () => {
  const writes: string[] = []
  const stdout = {
    columns: 120,
    rows: 24,
    write: (chunk: string) => {
      writes.push(chunk)
      return true
    },
  }
  const mockProcess = { stdout, stderr: { write: () => true } }

  const cwd = '/home/jane/project'
  const streamParser = createStreamParser()
  const stop = initDefaultReporter({
    streamParser: streamParser as StreamParser<logs.Log>,
    reportingOptions: { throttleProgress: 0 },
    context: {
      argv: ['install'],
      config: { dir: cwd } as ReporterPnpmConfig,
      process: mockProcess as unknown as NodeJS.Process,
    },
  })

  try {
    await yieldTick()

    stageLogger.debug({ prefix: cwd, stage: 'resolution_started' })
    progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'resolved' })
    await waitFor(writes, w => w.some(s => stripAnsi(s).includes('resolved 1')))

    stdout.columns = 40
    const writesBeforeResize = writes.length
    progressLogger.debug({ packageId: 'registry.npmjs.org/bar/1.0.0', requester: cwd, status: 'resolved' })
    await waitFor(writes, w => w.length > writesBeforeResize)

    const afterResize = writes.slice(writesBeforeResize).join('')
    expect(stripAnsi(afterResize)).toContain('resolved 2')
    expect(cursorUps(afterResize)).toEqual([])
  } finally {
    stop()
  }
})

// A frame taller than the terminal has scrolled its own top away, so nothing in
// it can be revised afterwards — not even once the terminal is big enough to
// hold the next frame. It has to be drawn afresh below instead.
test('never revises a frame that outgrew the terminal', async () => {
  const writes: string[] = []
  const stdout = {
    columns: 20,
    rows: 4,
    write: (chunk: string) => {
      writes.push(chunk)
      return true
    },
  }
  const mockProcess = { stdout, stderr: { write: () => true } }

  const cwd = '/home/jane/project'
  const streamParser = createStreamParser()
  const stop = initDefaultReporter({
    streamParser: streamParser as StreamParser<logs.Log>,
    reportingOptions: { throttleProgress: 0 },
    context: {
      argv: ['install'],
      config: { dir: cwd } as ReporterPnpmConfig,
      process: mockProcess as unknown as NodeJS.Process,
    },
  })

  try {
    await yieldTick()

    // One progress line, wrapping to more rows than this terminal has: the
    // requester is zoomed out into a prefix, so the line is well over 20 columns.
    const requester = `${cwd}/packages/a-fairly-long-workspace-package-name`
    stageLogger.debug({ prefix: requester, stage: 'resolution_started' })
    progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester, status: 'resolved' })
    await waitFor(writes, w => w.some(s => stripAnsi(s).includes('resolved 1')))

    // The terminal grows enough for the next frame to fit, but the one on
    // screen has already scrolled.
    stdout.rows = 24
    const writesBeforeGrow = writes.length
    progressLogger.debug({ packageId: 'registry.npmjs.org/bar/1.0.0', requester, status: 'resolved' })
    await waitFor(writes, w => w.length > writesBeforeGrow)

    const afterGrow = writes.slice(writesBeforeGrow).join('')
    expect(stripAnsi(afterGrow)).toContain('resolved 2')
    expect(cursorUps(afterGrow)).toEqual([])
  } finally {
    stop()
  }
})

// The window can shrink under a frame that fitted when it was drawn. Its top
// has scrolled away just as surely as an over-tall line's, so it cannot be
// revised either — including by the handover that commits the overflow.
test('a shrinking window starts a fresh frame', async () => {
  const writes: string[] = []
  const stdout = {
    columns: 120,
    rows: 24,
    write: (chunk: string) => {
      writes.push(chunk)
      return true
    },
  }
  const mockProcess = { stdout, stderr: { write: () => true } }

  const cwd = '/home/jane/project'
  const streamParser = createStreamParser()
  const stop = initDefaultReporter({
    streamParser: streamParser as StreamParser<logs.Log>,
    reportingOptions: { throttleProgress: 0 },
    context: {
      argv: ['update'],
      config: { dir: cwd } as ReporterPnpmConfig,
      process: mockProcess as unknown as NodeJS.Process,
    },
  })

  const groupDir = (group: number): string => `${cwd}/global/install-${group}`

  try {
    await yieldTick()

    const groupCount = 20
    for (let group = 0; group < groupCount; group++) {
      stageLogger.debug({ prefix: groupDir(group), stage: 'resolution_started' })
      progressLogger.debug({
        packageId: 'registry.npmjs.org/foo/1.0.0',
        requester: groupDir(group),
        status: 'resolved',
      })
    }
    await waitFor(writes, w => w.some(s => stripAnsi(s).includes(`install-${groupCount - 1}`)))

    stdout.rows = 6
    const writesBeforeShrink = writes.length
    progressLogger.debug({
      packageId: 'registry.npmjs.org/bar/1.0.0',
      requester: groupDir(0),
      status: 'resolved',
    })
    await waitFor(writes, w => w.length > writesBeforeShrink)

    const afterShrink = writes.slice(writesBeforeShrink).join('')
    expect(Math.max(0, ...cursorUps(afterShrink))).toBeLessThan(stdout.rows)
  } finally {
    stop()
  }
})
