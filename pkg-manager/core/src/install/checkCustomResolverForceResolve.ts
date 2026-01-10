import { parse as parseDepPath } from '@pnpm/dependency-path'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type CustomResolver, type WantedDependency, checkCustomResolverCanResolve } from '@pnpm/hooks.types'

export async function checkCustomResolverForceResolve (
  customResolvers: CustomResolver[],
  wantedLockfile: LockfileObject
): Promise<boolean> {
  if (!wantedLockfile.packages) return false

  for (const depPath of Object.keys(wantedLockfile.packages)) {
    const { name: alias } = parseDepPath(depPath)
    if (!alias) continue

    const wantedDependency: WantedDependency = { alias, bareSpecifier: depPath }

    for (const customResolver of customResolvers) {
      // eslint-disable-next-line no-await-in-loop
      const canResolve = await checkCustomResolverCanResolve(customResolver, wantedDependency)
      if (canResolve && customResolver.shouldForceResolve) {
        // eslint-disable-next-line no-await-in-loop
        if (await customResolver.shouldForceResolve(wantedDependency, wantedLockfile)) {
          return true
        }
      }
    }
  }

  return false
}
