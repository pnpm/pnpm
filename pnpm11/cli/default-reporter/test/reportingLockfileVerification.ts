import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { lockfileVerificationLogger, progressLogger, stageLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom, take, toArray } from 'rxjs'

test('prints lockfile verification in-progress and completion messages', async () => {
  const cwd = '/repo'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: cwd } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  // Subscribe before emitting so we capture both the started and the
  // done frame in the interactive (non-append-only) reporter.
  const frames = firstValueFrom(output$.pipe(take(2), toArray()))

  const lockfilePath = path.join(cwd, 'pnpm-lock.yaml')
  lockfileVerificationLogger.debug({ status: 'started', entries: 234, lockfilePath })
  lockfileVerificationLogger.debug({
    status: 'done',
    entries: 234,
    elapsedMs: 1234,
    lockfilePath,
  })

  const [started, done] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile against supply-chain policies (234 entries)...')
  expect(stripAnsi(done)).toBe('✓ Lockfile passes supply-chain policies (234 entries in 1.2s)')
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
  expect(stripAnsi(started)).toBe('? Verifying lockfile against supply-chain policies (1 entry)...')
  expect(stripAnsi(done)).toBe('✓ Lockfile passes supply-chain policies (1 entry in 42ms)')
})

test('prints relative path when lockfile lives outside the workspace root', async () => {
  const cwd = '/repo/packages/app'
  const workspaceDir = '/repo'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: cwd, workspaceDir } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(2), toArray()))

  // Lockfile lives in a sibling dir, not at the workspace root.
  const lockfilePath = '/repo/locks/pnpm-lock.yaml'
  lockfileVerificationLogger.debug({ status: 'started', entries: 5, lockfilePath })
  lockfileVerificationLogger.debug({
    status: 'done',
    entries: 5,
    elapsedMs: 200,
    lockfilePath,
  })

  const [started, done] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile at ../../locks/pnpm-lock.yaml against supply-chain policies (5 entries)...')
  expect(stripAnsi(done)).toBe('✓ Lockfile at ../../locks/pnpm-lock.yaml passes supply-chain policies (5 entries in 200ms)')
})

test('does not print path when running from workspace subdir and lockfile is at workspace root', async () => {
  const cwd = '/repo/packages/app'
  const workspaceDir = '/repo'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: cwd, workspaceDir } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(1), toArray()))

  const lockfilePath = path.join(workspaceDir, 'pnpm-lock.yaml')
  lockfileVerificationLogger.debug({ status: 'started', entries: 10, lockfilePath })

  const [started] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile against supply-chain policies (10 entries)...')
})

test('suppresses path when workspaceDir has a trailing separator', async () => {
  const cwd = '/repo'
  // Workspace dir with a trailing slash — strict === against
  // path.dirname(lockfilePath) would mismatch; path.relative normalizes.
  const workspaceDir = '/repo/'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: cwd, workspaceDir } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(1), toArray()))

  lockfileVerificationLogger.debug({
    status: 'started',
    entries: 3,
    lockfilePath: '/repo/pnpm-lock.yaml',
  })

  const [started] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile against supply-chain policies (3 entries)...')
})

test('prints a previously-verified line when the cached verdict is reused', async () => {
  const cwd = '/repo'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: cwd } as Config & ConfigContext,
    },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(1), toArray()))

  lockfileVerificationLogger.debug({
    status: 'cached',
    verifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
    lockfilePath: path.join(cwd, 'pnpm-lock.yaml'),
  })

  const [cached] = await frames
  expect(stripAnsi(cached)).toBe('✓ Lockfile passes supply-chain policies (verified 2h ago)')
})

test('falls back to a timeless cached message when the record has no timestamp', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(1), toArray()))

  lockfileVerificationLogger.debug({ status: 'cached' })

  const [cached] = await frames
  expect(stripAnsi(cached)).toBe('✓ Lockfile passes supply-chain policies (previously verified)')
})

test('emits a brief failure line on failed status', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const frames = firstValueFrom(output$.pipe(take(2), toArray()))

  lockfileVerificationLogger.debug({ status: 'started', entries: 12 })
  lockfileVerificationLogger.debug({
    status: 'failed',
    entries: 12,
    elapsedMs: 800,
  })

  const [started, failed] = await frames
  expect(stripAnsi(started)).toBe('? Verifying lockfile against supply-chain policies (12 entries)...')
  expect(stripAnsi(failed)).toBe('✗ Lockfile failed supply-chain policy check (12 entries in 800ms)')
})

// Regression test for https://github.com/pnpm/pnpm/issues — the cached
// verdict line was re-rendered on every subsequent progress tick because
// `mergeOutputs` keeps every non-fixed block in `acc.blocks` for the rest
// of the command. Each redraw re-includes the cached line, and any captured
// output (CI logs, `tee`, `script`, terminal scrollback) records each
// redraw as a separate line — producing dozens of copies of the same
// "Lockfile passes supply-chain policies (verified Nh ago)" message for a
// single underlying emission.
//
// The fix is to make the cached verdict a one-shot frame: render once,
// then clear so subsequent progress redraws don't re-include it.
test('cached verdict does not repeat on subsequent progress frames', async () => {
  const cwd = '/repo'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: cwd } as Config & ConfigContext,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  const lockfilePath = path.join(cwd, 'pnpm-lock.yaml')

  // Subscribe before emitting so the take(5) captures every frame from the
  // first emission onward — matching the pattern the other tests in this
  // file use and avoiding reliance on the `setTimeout(0)` deferral inside
  // `initDefaultReporter`.
  const frames = firstValueFrom(output$.pipe(take(5), toArray()))

  // One underlying emission — what the user actually wants to see once.
  lockfileVerificationLogger.debug({
    status: 'cached',
    verifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
    lockfilePath,
  })

  // Subsequent progress events from a different reporter source. Each
  // updates the rolling region of the combined frame; without the fix
  // each redraw re-includes the cached line that's still sitting in
  // `acc.blocks`.
  stageLogger.debug({ prefix: cwd, stage: 'resolution_started' })
  progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'resolved' })
  progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'fetched' })
  progressLogger.debug({ packageId: 'registry.npmjs.org/bar/1.0.0', requester: cwd, status: 'found_in_store' })
  progressLogger.debug({ method: 'hardlink', requester: cwd, status: 'imported', to: '/node_modules/.pnpm/bar@1.0.0' })

  const captured = await frames

  // Count how many of the captured frames still contain the cached line.
  // Before the fix this is 5 (one per frame); the fix renders the line
  // once and then clears it, so subsequent progress-only frames no longer
  // re-include it.
  const cachedFrameCount = captured.filter((frame) => frame.includes('Lockfile passes supply-chain policies')).length
  expect(cachedFrameCount).toBe(1)
})

// The cached-then-clear pair drives a fixed-block deletion through
// `mergeOutputs`' scan state. The clear emission carries an empty `msg`,
// which — once the cached line was the only block — produces a combined
// frame of "". That empty frame must NOT reach `logUpdate`, which
// unconditionally appends EOL and would write a visible blank line in
// captured TTY output (`script`, CI TTY captures). The scan still runs
// (state updates regardless); only the rendered empty frame is dropped.
test('cached verdict clear does not emit an empty frame', async () => {
  const cwd = '/repo'
  const output$ = toOutput$({
    context: { argv: ['install'], config: { dir: cwd } as Config & ConfigContext },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  // Subscribe before emitting so we capture every frame from the start.
  // `take(2)` because only two frames reach the subscriber: the cached
  // verdict and the first progress. The empty clear frame is dropped by
  // `mergeOutputs` (the fix under test), so `take(3)` would never
  // complete.
  const frames = firstValueFrom(output$.pipe(take(2), toArray()))

  lockfileVerificationLogger.debug({
    status: 'cached',
    verifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
    lockfilePath: path.join(cwd, 'pnpm-lock.yaml'),
  })
  // Stage + progress flush the cached-then-clear pair through the pipeline
  // and produce subsequent non-empty frames.
  stageLogger.debug({ prefix: cwd, stage: 'resolution_started' })
  progressLogger.debug({ packageId: 'registry.npmjs.org/foo/1.0.0', requester: cwd, status: 'resolved' })

  const captured = await frames
  expect(captured.includes('')).toBe(false)
})
