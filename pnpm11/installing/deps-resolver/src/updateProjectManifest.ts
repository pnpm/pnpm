import {
  getSpecFromPackageManifest,
  guessDependencyType,
  type PackageSpecObject,
  updateProjectManifestObject,
} from '@pnpm/pkg-manifest.utils'
import type { ProjectManifest } from '@pnpm/types'

import type { ImporterToResolve } from './index.js'
import type { ResolvedDirectDependency } from './resolveDependencyTree.js'

interface UpdateProjectManifestOptions {
  directDependencies: ResolvedDirectDependency[]
  preserveWorkspaceProtocol: boolean
  saveWorkspaceProtocol: boolean | 'rolling'
}

export async function updateProjectManifest (
  importer: ImporterToResolve,
  opts: UpdateProjectManifestOptions
): Promise<Array<ProjectManifest | undefined>> {
  if (!importer.manifest) {
    throw new Error('Cannot save because no package.json found')
  }
  const { specsToUpsert, declaredSpecifiers } = getSpecsOfResolvedDirectDependencies(importer, opts)
  addSpecsOfUnresolvedUpdates(importer, specsToUpsert)
  const hookedManifest = await updateProjectManifestObject(
    importer.rootDir,
    importer.manifest,
    specsToUpsert
  )
  const originalManifest = (importer.originalManifest != null)
    ? await updateProjectManifestObject(
      importer.rootDir,
      importer.originalManifest,
      applyDeclaredSpecifiers(specsToUpsert, declaredSpecifiers)
    )
    : undefined
  return [hookedManifest, originalManifest]
}

function getSpecsOfResolvedDirectDependencies (
  importer: ImporterToResolve,
  opts: Pick<UpdateProjectManifestOptions, 'directDependencies' | 'preserveWorkspaceProtocol'>
): { specsToUpsert: PackageSpecObject[], declaredSpecifiers: Map<string, string> } {
  const specsToUpsert: PackageSpecObject[] = []
  const declaredSpecifiers = new Map<string, string>()
  for (const rdd of opts.directDependencies) {
    const wantedDep = rdd.wantedDependency
    if (wantedDep?.updateSpec !== true || wantedDep.saveSpec === false) continue
    if (!belongsInTheProjectManifest(importer, rdd.alias, wantedDep.isNew)) continue
    const declaredSpecifier = getDeclaredSpecifierOwnedByHook(importer, rdd)
    if (declaredSpecifier != null) {
      declaredSpecifiers.set(rdd.alias, declaredSpecifier)
    }
    specsToUpsert.push({
      alias: rdd.alias,
      peer: isPeerOfImporter(importer, rdd.alias),
      bareSpecifier: declaredSpecifier == null
        ? getBareSpecifierToSave(wantedDep, rdd, opts.preserveWorkspaceProtocol)
        : wantedDep.bareSpecifier,
      resolvedVersion: rdd.version,
      rangeSpecStyle: importer.rangeSpecStyle,
      saveType: importer.targetDependenciesField,
    })
  }
  return { specsToUpsert, declaredSpecifiers }
}

function isPeerOfImporter (importer: ImporterToResolve, alias: string): boolean | undefined {
  return importer.peerAliases?.has(alias) ?? importer.peer
}

function applyDeclaredSpecifiers (
  specsToUpsert: PackageSpecObject[],
  declaredSpecifiers: Map<string, string>
): PackageSpecObject[] {
  if (declaredSpecifiers.size === 0) return specsToUpsert
  return specsToUpsert.map((spec) => declaredSpecifiers.has(spec.alias)
    ? { ...spec, bareSpecifier: declaredSpecifiers.get(spec.alias) }
    : spec)
}

// Re-save a dependency flagged for update that failed to resolve (e.g. a
// missing optional, hence absent from `directDependencies`) carrying no
// specifier, so it keeps its existing version under the importer's target
// field (which is unset for a plain install/update, making this a no-op).
function addSpecsOfUnresolvedUpdates (
  importer: ImporterToResolve,
  specsToUpsert: PackageSpecObject[]
): void {
  for (const pkgToInstall of importer.wantedDependencies) {
    if (
      pkgToInstall.updateSpec &&
      pkgToInstall.saveSpec !== false &&
      pkgToInstall.alias &&
      belongsInTheProjectManifest(importer, pkgToInstall.alias, pkgToInstall.isNew) &&
      !specsToUpsert.some(({ alias }) => alias === pkgToInstall.alias)
    ) {
      specsToUpsert.push({
        alias: pkgToInstall.alias,
        peer: isPeerOfImporter(importer, pkgToInstall.alias),
        saveType: importer.targetDependenciesField,
      })
    }
  }
}

/**
 * Whether the upsert belongs in the manifest the project keeps on disk. A
 * dependency a hook injected is declared only in the manifest resolution ran
 * against, so writing it back would hand the project a dependency it never
 * asked for. A dependency this run adds (`pnpm add foo`) is the project's from
 * now on, whether or not a hook already supplied it.
 */
function belongsInTheProjectManifest (
  importer: ImporterToResolve,
  alias: string,
  isNew: boolean | undefined
): boolean {
  return isNew === true ||
    importer.originalManifest == null ||
    guessDependencyType(alias, importer.originalManifest) != null
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
  if (rdd.wantedDependency?.saveSpec === true || importer.originalManifest == null) return undefined
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
