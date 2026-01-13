import { type LockfileObject } from '@pnpm/lockfile.types'
import { type CustomResolver, type WantedDependency, checkCustomResolverCanResolve } from '@pnpm/hooks.types'
import { parse as parseDepPath } from '@pnpm/dependency-path'

// Sentinel for Promise.any rejections (not an error condition)
const SKIP = new Error('skip')

export async function checkCustomResolverForceResolve (
  customResolvers: CustomResolver[],
  wantedLockfile: LockfileObject
): Promise<boolean> {
  if (!wantedLockfile.packages) return false

  const resolversWithHook = customResolvers.filter(resolver => resolver.shouldForceResolve)
  if (resolversWithHook.length === 0) return false

  // Run shouldForceResolve checks in parallel
  const pendingForceResolveChecks = Object.entries(wantedLockfile.packages).flatMap(([depPath, pkgSnapshot]) => {
    const { name: alias, version, nonSemverVersion } = parseDepPath(depPath)
    if (!alias) return []

    const wantedDependency: WantedDependency = {
      alias,
      bareSpecifier: version ?? nonSemverVersion,
    }

    return resolversWithHook.map(async resolver => {
      const canResolve = await checkCustomResolverCanResolve(resolver, wantedDependency)
      if (!canResolve) return Promise.reject(SKIP)
      const result = await resolver.shouldForceResolve!(depPath, pkgSnapshot)
      return result ? true : Promise.reject(SKIP)
    })
  })

  // Return true immediately if any check resolves as true; otherwise, return false
  try {
    return await Promise.any(pendingForceResolveChecks)
  } catch {
    return false
  }
}
