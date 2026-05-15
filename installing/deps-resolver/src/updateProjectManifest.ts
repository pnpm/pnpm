import {
  type PackageSpecObject,
  updateProjectManifestObject,
} from '@pnpm/pkg-manifest.utils'
import type { ProjectManifest } from '@pnpm/types'

import type { ImporterToResolve } from './index.js'
import type { ResolvedDirectDependency } from './resolveDependencyTree.js'

export async function updateProjectManifest (
  importer: ImporterToResolve,
  opts: {
    directDependencies: ResolvedDirectDependency[]
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: boolean | 'rolling'
  }
): Promise<Array<ProjectManifest | undefined>> {
  if (!importer.manifest) {
    throw new Error('Cannot save because no package.json found')
  }
  const specsToUpsert: PackageSpecObject[] = opts.directDependencies
    .filter((rdd) => importer.wantedDependencies.some((wd) => wd.alias === rdd.alias && wd.updateSpec))
    .map((rdd) => {
      // NOTE: directDependencies and wantedDependencies are not aligned by index
      // because linked dependencies (e.g. workspace:*) are excluded from
      // directDependencies. Use rdd.alias to find the correct wantedDependency
      // instead of relying on array indices.
      // This fixes a bug where catalog: references were replaced with resolved
      // version numbers due to index misalignment.
      // See: https://github.com/pnpm/pnpm/issues/11658
      const wantedDep = importer.wantedDependencies.find((wd) => wd.alias === rdd.alias)!
      return {
        alias: rdd.alias,
        peer: importer.peer,
        bareSpecifier: rdd.catalogLookup?.userSpecifiedBareSpecifier ?? rdd.normalizedBareSpecifier ?? wantedDep.bareSpecifier,
        resolvedVersion: rdd.version,
        pinnedVersion: importer.pinnedVersion,
        saveType: importer.targetDependenciesField,
      }
    })
  for (const pkgToInstall of importer.wantedDependencies) {
    if (pkgToInstall.updateSpec && pkgToInstall.alias && !specsToUpsert.some(({ alias }) => alias === pkgToInstall.alias)) {
      specsToUpsert.push({
        alias: pkgToInstall.alias,
        peer: importer.peer,
        saveType: importer.targetDependenciesField,
      })
    }
  }
  const hookedManifest = await updateProjectManifestObject(
    importer.rootDir,
    importer.manifest,
    specsToUpsert
  )
  const originalManifest = (importer.originalManifest != null)
    ? await updateProjectManifestObject(
      importer.rootDir,
      importer.originalManifest,
      specsToUpsert
    )
    : undefined
  return [hookedManifest, originalManifest]
}
