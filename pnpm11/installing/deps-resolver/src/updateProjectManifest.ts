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
  // Pair each resolved direct dependency with the wanted dependency it was
  // resolved from. A wanted dependency that carries an alias is matched by
  // that alias, so a dependency that failed to resolve (e.g. an optional one)
  // and dropped out of `directDependencies` cannot shift the pairing onto an
  // unrelated dependency (https://github.com/pnpm/pnpm/issues/11267).
  // Aliasless wanted dependencies — `pnpm add ./local`, `pnpm add jsr:@x/y`,
  // a bare `owner/repo#sha`, a GitHub URL — resolve to an alias that no wanted
  // dependency declared, so they are paired with the remaining resolved
  // dependencies in order.
  const wantedDepsByAlias = new Map<string, ImporterToResolve['wantedDependencies'][number]>()
  const aliaslessWantedDeps: Array<ImporterToResolve['wantedDependencies'][number]> = []
  for (const wantedDep of importer.wantedDependencies) {
    if (wantedDep.alias) {
      wantedDepsByAlias.set(wantedDep.alias, wantedDep)
    } else if (wantedDep.updateSpec) {
      aliaslessWantedDeps.push(wantedDep)
    }
  }
  let nextAliaslessIndex = 0
  const specsToUpsert: PackageSpecObject[] = []
  for (const rdd of opts.directDependencies) {
    const wantedDep = wantedDepsByAlias.has(rdd.alias)
      ? wantedDepsByAlias.get(rdd.alias)
      : aliaslessWantedDeps[nextAliaslessIndex++]
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
  wantedDep: ImporterToResolve['wantedDependencies'][number],
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
