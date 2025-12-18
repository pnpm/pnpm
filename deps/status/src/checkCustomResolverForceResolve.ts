import { type ProjectManifest } from '@pnpm/types'
import { type LockfileObject } from '@pnpm/lockfile.fs'
import { type CustomResolver, type WantedDependency, checkCustomResolverCanResolve } from '@pnpm/hooks.types'

export async function checkCustomResolverForceResolve (
  customResolvers: CustomResolver[],
  wantedLockfile: LockfileObject,
  manifests: ProjectManifest[]
): Promise<boolean> {
  for (const manifest of manifests) {
    const allDeps = {
      ...manifest.dependencies,
      ...manifest.devDependencies,
      ...manifest.optionalDependencies,
      ...manifest.peerDependencies,
    }

    for (const [depName, bareSpec] of Object.entries(allDeps)) {
      const wantedDependency: WantedDependency = {
        alias: depName,
        bareSpecifier: bareSpec,
      }

      for (const customResolver of customResolvers) {
        // eslint-disable-next-line no-await-in-loop
        const canResolve = await checkCustomResolverCanResolve(customResolver, wantedDependency)

        if (canResolve && customResolver.shouldForceResolve) {
          // eslint-disable-next-line no-await-in-loop
          const shouldForce = await customResolver.shouldForceResolve(wantedDependency, wantedLockfile)

          if (shouldForce) {
            return true
          }
        }
      }
    }
  }

  return false
}
