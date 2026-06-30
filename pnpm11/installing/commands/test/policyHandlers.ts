import { expect, jest, test } from '@jest/globals'

import { type PolicyViolation, setupPolicyHandlers } from '../lib/policyHandlers.js'

function violation (
  name: string,
  version: string,
  code = 'MINIMUM_RELEASE_AGE_VIOLATION'
): PolicyViolation {
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

test('setupPolicyHandlers returns undefined when no policy is active', () => {
  expect(setupPolicyHandlers({})).toBeUndefined()
})

test('setupPolicyHandlers returns a plan even when strict mode is on without a TTY', () => {
  // Pre-refactor this returned undefined and the resolver did the fail-fast
  // throw. Now the plan is always returned: the strict-no-TTY case throws
  // from the handler with the full violation list, not just the first
  // immature pick the resolver happened to hit.
  withStdinTTY(false, () => {
    expect(setupPolicyHandlers({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })).toBeDefined()
  })
})

test('strict no-TTY plan throws from the hook with the full violation list', async () => {
  await withStdinTTY(false, async () => {
    const plan = setupPolicyHandlers({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      ci: false,
    })!
    await expect(plan.handleResolutionPolicyViolations([
      violation('foo', '1.0.0'),
      violation('bar', '2.3.4'),
    ])).rejects.toMatchObject({
      code: 'ERR_PNPM_NO_MATURE_MATCHING_VERSION',
    })
  })
})

test('setupPolicyHandlers returns a plan when ci=false and stdin is a TTY', () => {
  withStdinTTY(true, () => {
    const plan = setupPolicyHandlers({
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
    const plan = setupPolicyHandlers({
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: true,
      save: false,
      ci: false,
    })!
    await expect(plan.handleResolutionPolicyViolations([violation('foo', '1.0.0')]))
      .rejects.toMatchObject({ code: 'ERR_PNPM_STRICT_MIN_RELEASE_AGE_REQUIRES_SAVE' })
  })
})

test('loose + --no-save runs the hook as a no-op (lockfile re-triggers auto-collect later)', async () => {
  // Loose mode never persists from the hook anyway — `pickManifestUpdates`
  // is what writes the exclude list at the install's tail, and the
  // installDeps / recursive `opts.save !== false` gates already skip that
  // when --no-save is set.
  const plan = setupPolicyHandlers({
    minimumReleaseAge: 60,
    save: false,
  })!
  await expect(plan.handleResolutionPolicyViolations([violation('foo', '1.0.0')]))
    .resolves.toBeUndefined()
})

test('loose-mode plan emits a workspace patch with sorted unique entries and logs once', () => {
  const plan = setupPolicyHandlers({ minimumReleaseAge: 60 })!
  const violations = [
    violation('foo', '1.0.0'),
    violation('foo', '1.0.0'),
    violation('bar', '2.3.4'),
    // Non-minimumReleaseAge code: the minimumReleaseAge handler ignores it.
    // (When more handlers register, each filters its own codes.)
    violation('quux', '0.0.1', 'TRUST_DOWNGRADE'),
  ]

  // Avoid leaking console output in test runs.
  const infoSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
  try {
    const updates = plan.pickManifestUpdates(violations)
    expect(updates).toEqual({ addedMinimumReleaseAgeExcludes: ['bar@2.3.4', 'foo@1.0.0'] })
  } finally {
    infoSpy.mockRestore()
  }
})

test('loose-mode plan combines multiple immature versions of one package into a single entry', () => {
  const plan = setupPolicyHandlers({ minimumReleaseAge: 60 })!
  const violations = [
    violation('foo', '2.0.0'),
    violation('foo', '1.0.0'),
    violation('bar', '3.1.0'),
  ]

  // Avoid leaking console output in test runs.
  const infoSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
  try {
    const updates = plan.pickManifestUpdates(violations)
    // The versions of `foo` collapse into one canonical, semver-sorted
    // entry — matching the form the workspace writer persists, so the
    // logged "Added N entries" count is not inflated by per-version picks.
    expect(updates).toEqual({ addedMinimumReleaseAgeExcludes: ['bar@3.1.0', 'foo@1.0.0 || 2.0.0'] })
  } finally {
    infoSpy.mockRestore()
  }
})

test('pickManifestUpdates returns undefined when no handler contributes anything', () => {
  const plan = setupPolicyHandlers({ minimumReleaseAge: 60 })!
  expect(plan.pickManifestUpdates([])).toBeUndefined()
  // Codes the minimumReleaseAge handler doesn't recognize don't produce a
  // patch — and with no other handler registered yet, the merged result
  // collapses to undefined so the install command skips the workspace
  // writer entirely.
  expect(plan.pickManifestUpdates([violation('foo', '1.0.0', 'TRUST_DOWNGRADE')])).toBeUndefined()
})

test('the hook is a no-op in loose mode regardless of violations', async () => {
  const plan = setupPolicyHandlers({ minimumReleaseAge: 60 })!
  // Loose mode never prompts — picks are persisted from
  // `pickManifestUpdates` at the end of the install.
  await expect(plan.handleResolutionPolicyViolations([violation('foo', '1.0.0')]))
    .resolves.toBeUndefined()
})
