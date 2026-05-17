import { expect, jest, test } from '@jest/globals'

import { type ResolverViolation, setupImmaturePicks } from '../lib/immaturePicks.js'

function violation (
  name: string,
  version: string,
  code = 'MINIMUM_RELEASE_AGE_VIOLATION'
): ResolverViolation {
  return { name, version, code, reason: 'stub reason' }
}

// Swap `process.stdin.isTTY` for the duration of a test, restoring the
// original descriptor — not just the value — so the property's
// configurability/enumerability shape doesn't leak between tests when
// the host process didn't define an own `isTTY` at all.
function withStdinTTY (value: boolean | undefined, fn: () => void | Promise<void>): void | Promise<void> {
  const originalDescriptor = Object.getOwnPropertyDescriptor(process.stdin, 'isTTY')
  Object.defineProperty(process.stdin, 'isTTY', { value, configurable: true, writable: true })
  const restore = (): void => {
    if (originalDescriptor) {
      Object.defineProperty(process.stdin, 'isTTY', originalDescriptor)
    } else {
      delete (process.stdin as { isTTY?: boolean }).isTTY
    }
  }
  let result: void | Promise<void>
  try {
    result = fn()
  } catch (err) {
    restore()
    throw err
  }
  if (result && typeof (result as Promise<void>).then === 'function') {
    return (result as Promise<void>).then(
      (v) => {
        restore(); return v
      },
      (err) => {
        restore(); throw err
      }
    )
  }
  restore()
  return result
}

test('setupImmaturePicks returns undefined when minimumReleaseAge is unset', () => {
  expect(setupImmaturePicks({})).toBeUndefined()
})

test('setupImmaturePicks returns a plan even when strict mode is on without a TTY', () => {
  // Pre-refactor this returned undefined and the resolver did the fail-fast
  // throw. Now the plan is always returned: the strict-no-TTY case throws
  // from `onAfterResolveDependencyTree` with the full violation list, not
  // just the first immature pick the resolver happened to hit.
  withStdinTTY(false, () => {
    expect(setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })).toBeDefined()
  })
})

test('strict no-TTY plan throws from onAfterResolveDependencyTree with the full violation list', async () => {
  await withStdinTTY(false, async () => {
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
  })
})

test('setupImmaturePicks returns a plan when ci=false and stdin is a TTY', () => {
  withStdinTTY(true, () => {
    const plan = setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })
    expect(plan).toBeDefined()
  })
})

test('strict + --no-save refuses up-front instead of prompting for approval it cannot persist', async () => {
  // The prompt promises to write to minimumReleaseAgeExclude, but the
  // install command's `opts.save !== false` gate blocks that under
  // --no-save — accepting the prompt would leave the lockfile holding
  // approved-but-unlisted picks that the next install rejects.
  await withStdinTTY(true, async () => {
    const plan = setupImmaturePicks({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      save: false,
      ci: false,
    })!
    await expect(plan.onAfterResolveDependencyTree([violation('foo', '1.0.0')]))
      .rejects.toMatchObject({ code: 'ERR_PNPM_STRICT_MIN_RELEASE_AGE_REQUIRES_SAVE' })
  })
})

test('loose + --no-save runs the hook as a no-op (lockfile re-triggers auto-collect later)', async () => {
  // Loose mode never persists from the hook anyway — `pickEntriesToPersist`
  // is what writes the exclude list at the install's tail, and the
  // installDeps / recursive `opts.save !== false` gates already skip that
  // when --no-save is set.
  const plan = setupImmaturePicks({
    minimumReleaseAge: 60,
    save: false,
  })!
  await expect(plan.onAfterResolveDependencyTree([violation('foo', '1.0.0')]))
    .resolves.toBeUndefined()
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
