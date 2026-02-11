import { type LockfileObject } from '@pnpm/lockfile.types'
import { type CustomResolver } from '@pnpm/hooks.types'

/**
 * Check if any custom resolver's shouldForceResolve returns true for any
 * package in the lockfile. shouldForceResolve is called independently of
 * canResolve — it runs before resolution, so the original specifier is not
 * available. Each resolver's shouldForceResolve is responsible for its own
 * filtering logic.
 */
export async function checkCustomResolverForceResolve (
  customResolvers: CustomResolver[],
  wantedLockfile: LockfileObject
): Promise<boolean> {
  if (!wantedLockfile.packages) return false

  const resolversWithHook = customResolvers.filter(resolver => resolver.shouldForceResolve)
  if (resolversWithHook.length === 0) return false

  const checks = Object.entries(wantedLockfile.packages).flatMap(([depPath, pkgSnapshot]) =>
    resolversWithHook.map(resolver => resolver.shouldForceResolve!(depPath, pkgSnapshot))
  )
  const results = await Promise.all(checks)
  return results.some(Boolean)
}
