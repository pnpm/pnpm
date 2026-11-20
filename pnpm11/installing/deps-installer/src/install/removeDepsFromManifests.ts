import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { removeDeps } from '../uninstall/removeDeps.js'
import type { UninstallSomeDepsMutation } from './mutationTypes.js'

/**
 * Drops the dependencies an `uninstallSome` mutation names from the project's
 * manifest and, when there is one, from its original manifest.
 */
export async function removeDepsFromManifests (
  project: { manifest: ProjectManifest, originalManifest?: ProjectManifest },
  mutation: UninstallSomeDepsMutation & { rootDir: ProjectRootDir }
): Promise<void> {
  const removeDepsFrom = async (manifest: ProjectManifest) => removeDeps(manifest, mutation.dependencyNames, { prefix: mutation.rootDir, saveType: mutation.targetDependenciesField })
  project.manifest = await removeDepsFrom(project.manifest)
  if (project.originalManifest != null) {
    project.originalManifest = await removeDepsFrom(project.originalManifest)
  }
}
