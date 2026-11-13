import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type DependenciesMeta,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'
import { equals } from 'ramda'

export function maybePruneMergedImporters (
  wantedLockfile: LockfileObject,
  preMergeImporters: LockfileObject['importers'] | undefined,
  projects: Array<{ id: ProjectId, manifest: ProjectManifest }>,
  autoInstallPeers: boolean
): LockfileObject {
  if (preMergeImporters == null) return wantedLockfile
  let prunedAnyImporter = false
  for (const project of projects) {
    prunedAnyImporter = pruneMergedDependencies({
      importer: wantedLockfile.importers[project.id],
      preMergeImporter: preMergeImporters[project.id],
      manifest: project.manifest,
      autoInstallPeers,
    }) || prunedAnyImporter
  }
  return prunedAnyImporter ? pruneSharedLockfile(wantedLockfile) : wantedLockfile
}

function pruneMergedDependencies (
  opts: {
    importer: ProjectSnapshot
    preMergeImporter: ProjectSnapshot | undefined
    manifest: ProjectManifest
    autoInstallPeers: boolean
  }
): boolean {
  const { importer, preMergeImporter } = opts
  const declaredDepNames = declaredDepNamesByField(opts.manifest, opts.autoInstallPeers)
  const depsPruned = pruneDependencyFields(importer, preMergeImporter, declaredDepNames)
  pruneSpecifiers(importer, preMergeImporter, declaredDepNames)
  reconcileImporterMetadata(importer, preMergeImporter, opts.manifest)
  return depsPruned
}

function reconcileImporterMetadata (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  manifest: ProjectManifest
): void {
  reconcilePublishDirectory(importer, preMergeImporter, manifest)
  reconcileLinkDirectory(importer, preMergeImporter, manifest)
  reconcileDependenciesMeta(importer, preMergeImporter, manifest)
}

function reconcilePublishDirectory (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  manifest: ProjectManifest
): void {
  const manifestPublishDir = manifest.publishConfig?.directory
  if (importer.publishDirectory === manifestPublishDir || preMergeImporter?.publishDirectory !== manifestPublishDir) {
    return
  }
  if (manifestPublishDir == null) {
    delete importer.publishDirectory
  } else {
    importer.publishDirectory = manifestPublishDir
  }
}

function reconcileLinkDirectory (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  manifest: ProjectManifest
): void {
  const manifestLinkDir = expectedLinkDirectory(manifest.publishConfig?.directory, manifest.publishConfig?.linkDirectory)
  const preLinkDir = expectedLinkDirectory(preMergeImporter?.publishDirectory, preMergeImporter?.linkDirectory)
  if (importer.linkDirectory === manifestLinkDir || preLinkDir !== manifestLinkDir) {
    return
  }
  if (manifestLinkDir === false) {
    importer.linkDirectory = false
  } else {
    delete importer.linkDirectory
  }
}

function expectedLinkDirectory (publishDirectory: string | undefined, linkDirectory: boolean | undefined): false | undefined {
  return publishDirectory != null && linkDirectory === false ? false : undefined
}

function reconcileDependenciesMeta (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  manifest: ProjectManifest
): void {
  const mergedMeta = importer.dependenciesMeta
  if (mergedMeta == null) return
  for (const depName of Object.keys(mergedMeta)) {
    reconcileSingleDepMeta(mergedMeta, depName, preMergeImporter?.dependenciesMeta, manifest.dependenciesMeta)
  }
  if (Object.keys(mergedMeta).length === 0) {
    delete importer.dependenciesMeta
  }
}

function reconcileSingleDepMeta (
  mergedMeta: DependenciesMeta,
  depName: string,
  preMeta: DependenciesMeta | undefined,
  manifestMeta: DependenciesMeta | undefined
): void {
  const manifestEntry = getOwnEntry(manifestMeta, depName)
  const preEntry = getOwnEntry(preMeta, depName)
  if (manifestEntry == null) {
    if (preEntry == null) {
      delete mergedMeta[depName]
    }
    return
  }
  reconcileDependencyMetaEntry(
    mergedMeta[depName] as Record<string, unknown>,
    preEntry as Record<string, unknown> | undefined,
    manifestEntry as Record<string, unknown>
  )
}

function getOwnEntry (meta: DependenciesMeta | undefined, depName: string): DependenciesMeta[string] | undefined {
  return meta != null && Object.hasOwn(meta, depName) ? meta[depName] : undefined
}

function reconcileDependencyMetaEntry (
  mergedProps: Record<string, unknown>,
  preProps: Record<string, unknown> | undefined,
  manifestProps: Record<string, unknown>
): void {
  for (const key of Object.keys(mergedProps)) {
    reconcileDependencyMetaProp(mergedProps, key, preProps, manifestProps)
  }
}

function reconcileDependencyMetaProp (
  mergedProps: Record<string, unknown>,
  key: string,
  preProps: Record<string, unknown> | undefined,
  manifestProps: Record<string, unknown>
): void {
  const hasManifestKey = hasDefinedOwnProp(manifestProps, key)
  const hasPreKey = preProps != null && hasDefinedOwnProp(preProps, key)
  if (!hasManifestKey) {
    if (!hasPreKey) {
      delete mergedProps[key]
    }
    return
  }
  if (!equals(mergedProps[key], manifestProps[key]) && hasPreKey && equals(preProps[key], manifestProps[key])) {
    mergedProps[key] = manifestProps[key]
  }
}

function hasDefinedOwnProp (record: Record<string, unknown>, key: string): boolean {
  return Object.hasOwn(record, key) && record[key] !== undefined
}

function pruneDependencyFields (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  declaredDepNames: Record<DependenciesField, Set<string>>
): boolean {
  let pruned = false
  for (const depField of DEPENDENCIES_FIELDS) {
    if (pruneSingleDependencyField(importer, depField, preMergeImporter?.[depField], declaredDepNames[depField])) {
      pruned = true
    }
  }
  return pruned
}

function pruneSingleDependencyField (
  importer: ProjectSnapshot,
  depField: DependenciesField,
  preMergeDeps: Record<string, string> | undefined,
  declaredNames: Set<string>
): boolean {
  const deps = importer[depField]
  if (deps == null) return false
  let pruned = false
  for (const depName of Object.keys(deps)) {
    if (!declaredNames.has(depName) && preMergeDeps?.[depName] == null) {
      delete deps[depName]
      pruned = true
    }
  }
  if (Object.keys(deps).length === 0) {
    delete importer[depField]
  }
  return pruned
}

function pruneSpecifiers (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  declaredDepNames: Record<DependenciesField, Set<string>>
): void {
  for (const depName of Object.keys(importer.specifiers)) {
    const isDeclared = DEPENDENCIES_FIELDS.some((depField) => declaredDepNames[depField].has(depName))
    if (!isDeclared && preMergeImporter?.specifiers?.[depName] == null) {
      delete importer.specifiers[depName]
    }
  }
}

// Mirrors how satisfiesPackageManifest assigns a manifest entry to a lockfile
// field, so that pruning to the manifest hands the frozen-lockfile check the
// same fields it derives for itself.
function declaredDepNamesByField (
  manifest: ProjectManifest,
  autoInstallPeers: boolean
): Record<DependenciesField, Set<string>> {
  const optionalDependencies = new Set(Object.keys(manifest.optionalDependencies ?? {}))
  const dependencies = new Set(Object.keys(manifest.dependencies ?? {})
    .filter((depName) => !optionalDependencies.has(depName)))
  const devDependencies = new Set(Object.keys(manifest.devDependencies ?? {})
    .filter((depName) => !optionalDependencies.has(depName) && !dependencies.has(depName)))
  if (autoInstallPeers) {
    // A peer another field declares is not auto-installed, so it stays where
    // that field puts it.
    for (const depName of Object.keys(manifest.peerDependencies ?? {})) {
      if (!optionalDependencies.has(depName) && !devDependencies.has(depName)) {
        dependencies.add(depName)
      }
    }
  }
  return { dependencies, devDependencies, optionalDependencies }
}
