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

  const resolversWithHook = customResolvers.filter(resolver => resolver.shouldRefreshResolution)
  if (resolversWithHook.length === 0) return false

  const checks = Object.entries(wantedLockfile.packages).flatMap(([depPath, pkgSnapshot]) =>
    resolversWithHook.map(resolver => resolver.shouldRefreshResolution!(depPath, pkgSnapshot))
  )
  const results = await Promise.all(checks)
  return results.some(Boolean)
}
