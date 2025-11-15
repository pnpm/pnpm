import { type ProjectId, type ProjectManifest } from '@pnpm/types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type Adapter, type PackageDescriptor, checkAdapterCanResolve } from '@pnpm/hooks.types'

export interface ProjectWithManifest {
  id: ProjectId
  manifest: ProjectManifest
}

/**
 * Check if any adapter requires force re-resolution for dependencies in the lockfile.
 * This is a pure function extracted from the install flow for testability.
 *
 * @param adapters - Array of adapters to check
 * @param wantedLockfile - Current lockfile
 * @param projects - Projects with their manifests
 * @returns Promise<boolean> - true if any adapter requires force re-resolution
 */
export async function checkAdapterForceResolve (
  adapters: Adapter[],
  wantedLockfile: LockfileObject,
  projects: ProjectWithManifest[]
): Promise<boolean> {
  for (const project of projects) {
    const allDeps = getAllDependenciesFromManifest(project.manifest)

    for (const [depName, range] of Object.entries(allDeps)) {
      const descriptor: PackageDescriptor = {
        name: depName,
        range,
      }

      for (const adapter of adapters) {
        // eslint-disable-next-line no-await-in-loop
        const canResolve = await checkAdapterCanResolve(adapter, descriptor)

        if (canResolve && adapter.shouldForceResolve) {
          // eslint-disable-next-line no-await-in-loop
          const shouldForce = await adapter.shouldForceResolve(descriptor)

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
