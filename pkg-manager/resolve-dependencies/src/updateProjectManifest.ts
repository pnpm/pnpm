import {
  type PackageSpecObject,
  updateProjectManifestObject,
} from '@pnpm/manifest-utils'
import { type ProjectManifest } from '@pnpm/types'
import { type ResolvedDirectDependency } from './resolveDependencyTree'
import { type ImporterToResolve } from '.'

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
    .filter((rdd, index) => importer.wantedDependencies[index]?.updateSpec)
    .map((rdd, index) => {
      const wantedDep = importer.wantedDependencies[index]!
      return {
        alias: rdd.alias,
        nodeExecPath: wantedDep.nodeExecPath,
        peer: importer.peer,
        bareSpecifier: rdd.catalogLookup?.userSpecifiedBareSpecifier ?? rdd.normalizedBareSpecifier ?? wantedDep.bareSpecifier,
        saveType: importer.targetDependenciesField,
      }
    })
  for (const pkgToInstall of importer.wantedDependencies) {
    if (pkgToInstall.updateSpec && pkgToInstall.alias && !specsToUpsert.some(({ alias }) => alias === pkgToInstall.alias)) {
      specsToUpsert.push({
        alias: pkgToInstall.alias,
        nodeExecPath: pkgToInstall.nodeExecPath,
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
