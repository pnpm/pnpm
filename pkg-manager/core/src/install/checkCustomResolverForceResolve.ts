import { type ProjectId, type ProjectManifest } from '@pnpm/types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type CustomResolver, type WantedDependency, checkCustomResolverCanResolve } from '@pnpm/hooks.types'

export interface ProjectWithManifest {
  id: ProjectId
  manifest: ProjectManifest
}

/**
 * Check if any custom resolver requires force re-resolution for dependencies in the lockfile.
 * This is a pure function extracted from the install flow for testability.
 *
 * @param customResolvers - Array of custom resolvers to check
 * @param wantedLockfile - Current lockfile
 * @param projects - Projects with their manifests
 * @returns Promise<boolean> - true if any custom resolver requires force re-resolution
 */
export async function checkCustomResolverForceResolve (
  customResolvers: CustomResolver[],
  wantedLockfile: LockfileObject,
  projects: ProjectWithManifest[]
): Promise<boolean> {
  for (const project of projects) {
    const allDeps = getAllDependenciesFromManifest(project.manifest)

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
          const shouldForce = await customResolver.shouldForceResolve(wantedDependency)

          if (shouldForce) {
            return true
          }
        }
      }
    }
  }

  return false
}

function getAllDependenciesFromManifest (manifest: {
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
  peerDependencies?: Record<string, string>
}): Record<string, string> {
  return {
    ...manifest.dependencies,
    ...manifest.devDependencies,
    ...manifest.optionalDependencies,
    ...manifest.peerDependencies,
  }
}
