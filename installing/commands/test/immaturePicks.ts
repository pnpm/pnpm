import { expect, jest, test } from '@jest/globals'

import { type ResolverViolation, setupImmaturePicks } from '../lib/immaturePicks.js'

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

test('setupImmaturePicks returns a plan even when strict mode is on without a TTY', () => {
  // Pre-refactor this returned undefined and the resolver did the fail-fast
  // throw. Now the plan is always returned: the strict-no-TTY case throws
  // from `onAfterResolveDependencyTree` with the full violation list, not
  // just the first immature pick the resolver happened to hit.
  const original = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: false, configurable: true })
  try {
    expect(setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })).toBeDefined()
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: original, configurable: true })
  }
})

test('strict no-TTY plan throws from onAfterResolveDependencyTree with the full violation list', async () => {
  const original = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: false, configurable: true })
  try {
    const plan = setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })!
    await expect(plan.onAfterResolveDependencyTree([
      violation('foo', '1.0.0'),
      violation('bar', '2.3.4'),
    ])).rejects.toMatchObject({
      code: 'ERR_PNPM_NO_MATURE_MATCHING_VERSION',
    })
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
  await expect(plan.onAfterResolveDependencyTree([violation('foo', '1.0.0')]))
    .resolves.toBeUndefined()
})
