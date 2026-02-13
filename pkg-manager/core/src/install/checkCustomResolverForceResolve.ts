import { type LockfileObject } from '@pnpm/lockfile.types'
import { type CustomResolver } from '@pnpm/hooks.types'

// Sentinel for Promise.any rejections (not an error condition)
const SKIP = new Error('skip')

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

  const checks = Object.entries(wantedLockfile.packages).flatMap(([depPath, pkgSnapshot]) =>
    hooks.map(async (shouldRefreshResolution) => {
      if (await shouldRefreshResolution(depPath, pkgSnapshot)) {
        return true
      }
      throw SKIP
    })
  )
  try {
    await Promise.any(checks)
    return true
  } catch (err) {
    if (!(err instanceof AggregateError)) throw err
    const realError = err.errors.find(e => e !== SKIP)
    if (realError) throw realError
    return false
  }
}
