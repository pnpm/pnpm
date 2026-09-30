import type { CustomResolver } from '@pnpm/hooks.types'
import type { LockfileObject } from '@pnpm/lockfile.types'

/**
 * Check if any custom resolver's shouldRefreshResolution returns true for any
 * package in the lockfile. shouldRefreshResolution is called independently of
 * canResolve — it runs before resolution, so the original specifier is not
 * available. Each resolver's shouldRefreshResolution is responsible for its own
 * filtering logic.
 */
export async function checkCustomResolverForceResolve (
  customResolvers: CustomResolver[],
  wantedLockfile: LockfileObject
): Promise<boolean> {
  if (!wantedLockfile.packages) return false

  const hooks = collectRefreshHooks(customResolvers)
  if (hooks.length === 0) return false

  const asyncChecks = runRefreshHooks(hooks, wantedLockfile.packages)
  if (asyncChecks === true) return true
  if (asyncChecks.length === 0) return false
  return anyTrue(asyncChecks)
}

type RefreshResolutionHook = NonNullable<CustomResolver['shouldRefreshResolution']>

function collectRefreshHooks (customResolvers: CustomResolver[]): RefreshResolutionHook[] {
  const hooks: RefreshResolutionHook[] = []
  for (const resolver of customResolvers) {
    if (resolver.shouldRefreshResolution) hooks.push(resolver.shouldRefreshResolution)
  }
  return hooks
}

/**
 * Returns `true` as soon as a hook synchronously requests a refresh.
 * Otherwise returns the pending asynchronous answers.
 */
function runRefreshHooks (
  hooks: RefreshResolutionHook[],
  packages: NonNullable<LockfileObject['packages']>
): true | Promise<boolean>[] {
  const asyncChecks: Promise<boolean>[] = []
  for (const [depPath, pkgSnapshot] of Object.entries(packages)) {
    for (const hook of hooks) {
      const result = hook(depPath, pkgSnapshot)
      if (result === true) {
        abandonChecks(asyncChecks)
        return true
      }
      if (result !== false) asyncChecks.push(result)
    }
  }
  return asyncChecks
}

/**
 * The answer is already known, so the outcome of the pending checks, a
 * failure included, no longer matters. Without a handler their rejection
 * would be unhandled.
 */
function abandonChecks (asyncChecks: Promise<boolean>[]): void {
  for (const check of asyncChecks) {
    check.catch(() => {})
  }
}

async function anyTrue (promises: Promise<boolean>[]): Promise<boolean> {
  return new Promise((resolve, reject) => {
    let remaining = promises.length
    if (remaining === 0) return resolve(false)
    const settleWith = (value: boolean): void => {
      if (value) resolve(true)
      else if (--remaining === 0) resolve(false)
    }
    for (const promise of promises) {
      promise.then(settleWith, reject)
    }
  })
}
