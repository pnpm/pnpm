import { type LockfileObject } from '@pnpm/lockfile.types'
import { type CustomResolver } from '@pnpm/hooks.types'

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

  const hooks: NonNullable<CustomResolver['shouldRefreshResolution']>[] = []
  for (const resolver of customResolvers) {
    if (resolver.shouldRefreshResolution) hooks.push(resolver.shouldRefreshResolution)
  }
  if (hooks.length === 0) return false

  const asyncChecks: Promise<boolean>[] = []
  for (const [depPath, pkgSnapshot] of Object.entries(wantedLockfile.packages)) {
    for (const hook of hooks) {
      const result = hook(depPath, pkgSnapshot)
      if (result === true) return true
      if (result !== false) asyncChecks.push(result)
    }
  }
  if (asyncChecks.length === 0) return false
  return anyTrue(asyncChecks)
}

async function anyTrue (promises: Promise<boolean>[]): Promise<boolean> {
  return new Promise((resolve, reject) => {
    let remaining = promises.length
    if (remaining === 0) return resolve(false)
    for (const p of promises) {
      p.then(value => {
        if (value) resolve(true)
        else if (--remaining === 0) resolve(false)
      }, reject)
    }
  })
}
