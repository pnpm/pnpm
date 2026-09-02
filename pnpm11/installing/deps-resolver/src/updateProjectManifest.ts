import {
  getSpecFromPackageManifest,
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
  const declaredSpecifiers = new Map<string, string>()
  for (const rdd of opts.directDependencies) {
    const wantedDep = rdd.wantedDependency
    if (wantedDep?.updateSpec !== true) continue
    const declaredSpecifier = getDeclaredSpecifierOwnedByHook(importer, rdd)
    if (declaredSpecifier != null) {
      declaredSpecifiers.set(rdd.alias, declaredSpecifier)
    }
    specsToUpsert.push({
      alias: rdd.alias,
      peer: importer.peer,
      bareSpecifier: declaredSpecifier == null
        ? getBareSpecifierToSave(wantedDep, rdd, opts.preserveWorkspaceProtocol)
        : wantedDep.bareSpecifier,
      resolvedVersion: rdd.version,
      rangeSpecStyle: importer.rangeSpecStyle,
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
      declaredSpecifiers.size === 0
        ? specsToUpsert
        : specsToUpsert.map((spec) => declaredSpecifiers.has(spec.alias)
          ? { ...spec, bareSpecifier: declaredSpecifiers.get(spec.alias) }
          : spec)
    )
    : undefined
  return [hookedManifest, originalManifest]
}

/**
 * The specifier the project declares on disk for a direct dependency an
 * override — or another `readPackage` hook — governs in the manifest handed to
 * the resolver. `undefined` when the declaration is what resolution followed,
 * and the update owns it.
 *
 * The version resolution settled on answers the override, not the declaration,
 * so neither manifest's entry is the update's to move: writing the resolved
 * range over them bakes the override into every project that declares the
 * package — replacing a `catalog:` reference with a version (pnpm/pnpm#12115)
 * — and leaves a specifier the hook rewrites away on the next install, which
 * `--frozen-lockfile` then rejects (pnpm/pnpm#14224).
 *
 * An override that repeats the declared range verbatim governs it just the
 * same, so a hook that rewrote nothing is recognized through
 * `isOverriddenDependency` rather than by comparing the two manifests.
 *
 * A dependency this run names with a specifier of its own (`pnpm add foo@2`)
 * is exempt: that request is the manifest's new specifier.
 */
function getDeclaredSpecifierOwnedByHook (
  importer: ImporterToResolve,
  rdd: ResolvedDirectDependency
): string | undefined {
  if (importer.originalManifest == null) return undefined
  const hookedSpecifier = getSpecFromPackageManifest(importer.manifest, rdd.alias)
  if (hookedSpecifier === '' || hookedSpecifier !== rdd.wantedDependency?.bareSpecifier) return undefined
  const declaredSpecifier = getSpecFromPackageManifest(importer.originalManifest, rdd.alias)
  if (declaredSpecifier === '') return undefined
  return declaredSpecifier !== hookedSpecifier || importer.isOverriddenDependency?.(rdd.alias, declaredSpecifier) === true
    ? declaredSpecifier
    : undefined
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

/**
 * Whether a `workspace:` specifier points at a directory rather than a range
 * (`workspace:../pkg`, not `workspace:^`). Such a path is resolved against the
 * project that declares it.
 */
export function isWorkspaceLocalPathSpecifier (bareSpecifier: string): boolean {
  if (!bareSpecifier.startsWith('workspace:')) return false
  const pref = bareSpecifier.slice('workspace:'.length)
  return pref.startsWith('.') || pref.startsWith('/') || pref.startsWith('~/') || /^[A-Z]:/i.test(pref)
}
