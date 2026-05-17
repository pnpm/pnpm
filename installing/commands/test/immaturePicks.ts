import { expect, jest, test } from '@jest/globals'

import { drainImmaturePicks, setupImmaturePicks } from '../lib/immaturePicks.js'

test('setupImmaturePicks returns undefined when minimumReleaseAge is unset', () => {
  expect(setupImmaturePicks({})).toBeUndefined()
})

test('setupImmaturePicks returns undefined when strict mode is on and stdin is not a TTY', () => {
  // Strict mode + non-TTY (CI) keeps the resolver's fail-fast behavior:
  // no prompt available, so a single throw on the first immature pick is
  // both deterministic and consistent with today's strict mode. Without a
  // resolution from this factory the resolver never receives
  // `deferImmatureDecision`, so its existing throw-on-first kicks in.
  const original = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: false, configurable: true })
  try {
    expect(setupImmaturePicks({ minimumReleaseAge: 60, minimumReleaseAgeStrict: true })).toBeUndefined()
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: original, configurable: true })
  }
})

test('setupImmaturePicks records and dedupes name@version entries in loose mode', () => {
  const setup = setupImmaturePicks({ minimumReleaseAge: 60 })!
  setup.collector.record({ name: 'foo', version: '1.0.0' })
  setup.collector.record({ name: 'foo', version: '1.0.0' })
  setup.collector.record({ name: 'bar', version: '2.3.4' })

  expect(setup.collector.versions).toEqual(new Set(['foo@1.0.0', 'bar@2.3.4']))
  // Loose-mode picks don't require approval — the auto-persist path
  // writes them to the workspace manifest at the end of the install.
  expect(setup.collector.promptRequired).toBe(false)
  expect(setup.deferImmatureDecision).toBe(true)
})

test('confirmImmaturePicks is a no-op in loose mode regardless of collector contents', async () => {
  const setup = setupImmaturePicks({ minimumReleaseAge: 60 })!
  setup.collector.record({ name: 'foo', version: '1.0.0' })

  // Loose mode never prompts. The resolution stays in flight; the install
  // proceeds and the workspace manifest write happens at the end.
  await expect(setup.confirmImmaturePicks()).resolves.toBeUndefined()
})

test('drainImmaturePicks returns sorted entries, clears the set, and logs once', () => {
  const setup = setupImmaturePicks({ minimumReleaseAge: 60 })!
  setup.collector.record({ name: 'foo', version: '1.0.0' })
  setup.collector.record({ name: 'bar', version: '2.3.4' })

  // Avoid leaking console output in test runs.
  const infoSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
  try {
    const drained = drainImmaturePicks(setup.collector)
    expect(drained).toEqual(['bar@2.3.4', 'foo@1.0.0'])
  } finally {
    infoSpy.mockRestore()
  }

  // Subsequent drain returns nothing — important so a follow-up install in
  // the same process doesn't re-announce entries already persisted.
  expect(drainImmaturePicks(setup.collector)).toBeUndefined()
})

test('drainImmaturePicks returns undefined for an empty or absent collector', () => {
  expect(drainImmaturePicks(undefined)).toBeUndefined()

  const setup = setupImmaturePicks({ minimumReleaseAge: 60 })!
  expect(drainImmaturePicks(setup.collector)).toBeUndefined()
})
