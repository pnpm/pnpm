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
  // directDependencies and wantedDependencies are not aligned by index
  // (linked deps like workspace:* are excluded from directDependencies),
  // so match by alias instead.
  const wantedDepsByAlias = new Map(
    importer.wantedDependencies.map((wd) => [wd.alias, wd] as const)
  )
  const specsToUpsert: PackageSpecObject[] = opts.directDependencies
    .filter((rdd) => wantedDepsByAlias.get(rdd.alias)?.updateSpec)
    .map((rdd) => {
      const wantedDep = wantedDepsByAlias.get(rdd.alias)!
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
