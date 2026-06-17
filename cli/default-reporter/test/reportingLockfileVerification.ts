import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { toOutput$ } from '@pnpm/cli.default-reporter'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { lockfileVerificationLogger } from '@pnpm/core-loggers'
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
