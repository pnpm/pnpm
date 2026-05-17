import { expect, jest, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'

import { type ResolverViolation, setupImmaturePicks } from '../lib/immaturePicks.js'

const STUB_LOCKFILE: LockfileObject = { lockfileVersion: '9.0', importers: {} }

function violation (
  name: string,
  version: string,
  code = 'MINIMUM_RELEASE_AGE_VIOLATION'
): ResolverViolation {
  return { name, version, code, reason: 'stub reason' }
}

test('setupImmaturePicks returns undefined when minimumReleaseAge is unset', () => {
  expect(setupImmaturePicks({})).toBeUndefined()
})

test('setupImmaturePicks returns undefined when strict mode is on and stdin is not a TTY', () => {
  const original = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: false, configurable: true })
  try {
    expect(setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })).toBeUndefined()
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: original, configurable: true })
  }
})

test('setupImmaturePicks returns undefined when ci=true even with stdin a TTY', () => {
  // Some CI runners allocate a TTY but still expect deterministic
  // non-interactive behavior. The `ci` option shuts the prompt off
  // independently of the TTY check.
  const original = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })
  try {
    expect(setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: true,
    })).toBeUndefined()
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: original, configurable: true })
  }
})

test('setupImmaturePicks returns a plan when ci=false and stdin is a TTY', () => {
  const original = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })
  try {
    const plan = setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })
    expect(plan).toBeDefined()
    expect(plan!.deferImmatureDecision).toBe(true)
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: original, configurable: true })
  }
})

test('loose-mode plan returns sorted unique name@version entries and logs once', () => {
  const plan = setupImmaturePicks({ minimumReleaseAge: 60 })!
  const violations = [
    violation('foo', '1.0.0'),
    violation('foo', '1.0.0'),
    violation('bar', '2.3.4'),
    violation('quux', '0.0.1', 'TRUST_DOWNGRADE'),
  ]

  // Avoid leaking console output in test runs.
  const infoSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
  try {
    const picked = plan.pickEntriesToPersist(violations)
    expect(picked).toEqual(['bar@2.3.4', 'foo@1.0.0'])
  } finally {
    infoSpy.mockRestore()
  }
})

test('pickEntriesToPersist returns undefined when no minimumReleaseAge violations are present', () => {
  const plan = setupImmaturePicks({ minimumReleaseAge: 60 })!
  expect(plan.pickEntriesToPersist([])).toBeUndefined()
  // Other policies' violations don't go on the minimumReleaseAge list.
  expect(plan.pickEntriesToPersist([violation('foo', '1.0.0', 'TRUST_DOWNGRADE')])).toBeUndefined()
})

test('onAfterResolveDependencyTree is a no-op in loose mode regardless of violations', async () => {
  const plan = setupImmaturePicks({ minimumReleaseAge: 60 })!
  // Loose mode never prompts — picks are persisted from
  // `pickEntriesToPersist` at the end of the install.
  await expect(plan.onAfterResolveDependencyTree([violation('foo', '1.0.0')], STUB_LOCKFILE))
    .resolves.toBeUndefined()
})
