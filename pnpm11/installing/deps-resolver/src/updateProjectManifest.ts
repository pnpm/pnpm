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
  const specsToUpsert: PackageSpecObject[] = []
  for (const rdd of opts.directDependencies) {
    const wantedDep = rdd.wantedDependency
    if (wantedDep?.updateSpec !== true) continue
    specsToUpsert.push({
      alias: rdd.alias,
      peer: importer.peer,
      bareSpecifier: getBareSpecifierToSave(wantedDep, rdd, opts.preserveWorkspaceProtocol),
      resolvedVersion: rdd.version,
      pinnedVersion: importer.pinnedVersion,
      saveType: importer.targetDependenciesField,
    })
  }
  // Re-save a dependency flagged for update that failed to resolve (e.g. a
  // missing optional, hence absent from `directDependencies`) carrying no
  // specifier, so it keeps its existing version under the importer's target
  // field (which is unset for a plain install/update, making this a no-op).
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

function getBareSpecifierToSave (
  wantedDep: { bareSpecifier: string },
  resolvedDep: ResolvedDirectDependency,
  preserveWorkspaceProtocol: boolean
): string {
  if (resolvedDep.catalogLookup != null) {
    return resolvedDep.catalogLookup.userSpecifiedBareSpecifier
  }
  if (preserveWorkspaceProtocol && isWorkspaceLocalPathSpecifier(wantedDep.bareSpecifier)) {
    return wantedDep.bareSpecifier
  }
  return resolvedDep.normalizedBareSpecifier ?? wantedDep.bareSpecifier
}

function isWorkspaceLocalPathSpecifier (bareSpecifier: string): boolean {
  if (!bareSpecifier.startsWith('workspace:')) return false
  const pref = bareSpecifier.slice('workspace:'.length)
  return pref.startsWith('.') || pref.startsWith('/') || pref.startsWith('~/') || /^[A-Z]:/i.test(pref)
}
