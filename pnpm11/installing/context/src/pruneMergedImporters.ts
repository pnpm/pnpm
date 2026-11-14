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

import { getDeclaredSpec, getLinkTarget, isLinkRef, restoreMovedLinks } from './movedLinks.js'

interface InjectedResolutionContext {
  manifest: ProjectManifest
  reconciledInjected: Set<string>
  preInjected: Set<string>
  mergedInjectedBefore: Set<string>
}

export function maybePruneMergedImporters (opts: {
  wantedLockfile: LockfileObject
  preMergeImporters: LockfileObject['importers'] | undefined
  projects: Array<{ id: ProjectId, manifest: ProjectManifest }>
  autoInstallPeers: boolean
}): LockfileObject {
  const { wantedLockfile, preMergeImporters, projects, autoInstallPeers } = opts
  if (preMergeImporters == null) return wantedLockfile
  const mergedLinkTargets = new Map<ProjectId, string>()
  for (const project of projects) {
    const importer = wantedLockfile.importers[project.id]
    if (importer != null) {
      mergedLinkTargets.set(project.id, getLinkTarget(project.id, importer))
    }
  }
  let prunedAnyImporter = false
  for (const project of projects) {
    prunedAnyImporter = pruneMergedDependencies({
      importer: wantedLockfile.importers[project.id],
      preMergeImporter: preMergeImporters[project.id],
      manifest: project.manifest,
      autoInstallPeers,
    }) || prunedAnyImporter
  }
  restoreMovedLinks({ wantedLockfile, preMergeImporters, projects, mergedLinkTargets })
  return prunedAnyImporter ? pruneSharedLockfile(wantedLockfile) : wantedLockfile
}

function pruneMergedDependencies (opts: {
  importer: ProjectSnapshot
  preMergeImporter: ProjectSnapshot | undefined
  manifest: ProjectManifest
  autoInstallPeers: boolean
}): boolean {
  const { importer, preMergeImporter, manifest, autoInstallPeers } = opts
  const declaredDepNames = declaredDepNamesByField(manifest, autoInstallPeers)
  const depsPruned = pruneDependencyFields(importer, preMergeImporter, declaredDepNames)
  pruneSpecifiers(importer, preMergeImporter, declaredDepNames)
  const edgesReconciled = reconcileImporterMetadata(importer, preMergeImporter, manifest)
  return depsPruned || edgesReconciled
}

function reconcileImporterMetadata (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  manifest: ProjectManifest
): boolean {
  const mergedInjectedBefore = getInjectedDepNames(importer.dependenciesMeta)
  reconcilePublishDirectory(importer, preMergeImporter, manifest)
  reconcileLinkDirectory(importer, preMergeImporter, manifest)
  reconcileDependenciesMeta(importer, preMergeImporter, manifest)
  return reconcileInjectedResolutions({
    importer,
    preMergeImporter,
    manifest,
    mergedInjectedBefore,
  })
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
    reconcileSingleDepMeta({
      mergedMeta,
      depName,
      preMeta: preMergeImporter?.dependenciesMeta,
      manifestMeta: manifest.dependenciesMeta,
    })
  }
  if (Object.keys(mergedMeta).length === 0) {
    delete importer.dependenciesMeta
  }
}

function reconcileSingleDepMeta (opts: {
  mergedMeta: DependenciesMeta
  depName: string
  preMeta: DependenciesMeta | undefined
  manifestMeta: DependenciesMeta | undefined
}): void {
  const { mergedMeta, depName, preMeta, manifestMeta } = opts
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
    reconcileDependencyMetaProp({ mergedProps, key, preProps, manifestProps })
  }
}

function reconcileDependencyMetaProp (opts: {
  mergedProps: Record<string, unknown>
  key: string
  preProps: Record<string, unknown> | undefined
  manifestProps: Record<string, unknown>
}): void {
  const { mergedProps, key, preProps, manifestProps } = opts
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

function getInjectedDepNames (meta: DependenciesMeta | undefined): Set<string> {
  const names = new Set<string>()
  if (meta == null) return names
  for (const depName of Object.keys(meta)) {
    if (getOwnEntry(meta, depName)?.injected === true) {
      names.add(depName)
    }
  }
  return names
}

function reconcileInjectedResolutions (opts: {
  importer: ProjectSnapshot
  preMergeImporter: ProjectSnapshot | undefined
  manifest: ProjectManifest
  mergedInjectedBefore: Set<string>
}): boolean {
  const { importer, preMergeImporter, manifest, mergedInjectedBefore } = opts
  if (preMergeImporter == null) return false
  const ctx: InjectedResolutionContext = {
    manifest,
    reconciledInjected: getInjectedDepNames(importer.dependenciesMeta),
    preInjected: getInjectedDepNames(preMergeImporter.dependenciesMeta),
    mergedInjectedBefore,
  }
  let changed = false
  for (const depField of DEPENDENCIES_FIELDS) {
    if (reconcileFieldInjectedResolutions({ importer, preMergeImporter, depField, ctx })) {
      changed = true
    }
  }
  return changed
}

function reconcileFieldInjectedResolutions (opts: {
  importer: ProjectSnapshot
  preMergeImporter: ProjectSnapshot
  depField: DependenciesField
  ctx: InjectedResolutionContext
}): boolean {
  const { importer, preMergeImporter, depField, ctx } = opts
  const deps = importer[depField]
  if (deps == null || preMergeImporter[depField] == null) return false
  let changed = false
  for (const depName of Object.keys(deps)) {
    if (reconcileDepInjectedResolution({ importer, preMergeImporter, depField, depName, ctx })) {
      changed = true
    }
  }
  return changed
}

function reconcileDepInjectedResolution (opts: {
  importer: ProjectSnapshot
  preMergeImporter: ProjectSnapshot
  depField: DependenciesField
  depName: string
  ctx: InjectedResolutionContext
}): boolean {
  const { importer, preMergeImporter, depField, depName, ctx } = opts
  const deps = importer[depField]!
  const preDeps = preMergeImporter[depField]!
  if (!Object.hasOwn(preDeps, depName) || !Object.hasOwn(preMergeImporter.specifiers, depName)) {
    return false
  }
  const preSpec = preMergeImporter.specifiers[depName]
  const expectedSpec = getDeclaredSpec(ctx.manifest, depField, depName) ?? importer.specifiers[depName]
  if (preSpec !== expectedSpec) return false
  const isInjected = ctx.reconciledInjected.has(depName)
  if (isInjected !== ctx.preInjected.has(depName)) return false
  if (!shouldRestorePreResolution({
    isInjected,
    wasMergedInjected: ctx.mergedInjectedBefore.has(depName),
    mergedRef: deps[depName],
    preRef: preDeps[depName],
  })) {
    return false
  }
  deps[depName] = preDeps[depName]
  importer.specifiers[depName] = preSpec
  return true
}

function shouldRestorePreResolution (opts: {
  isInjected: boolean
  wasMergedInjected: boolean
  mergedRef: string
  preRef: string
}): boolean {
  const { isInjected, wasMergedInjected, mergedRef, preRef } = opts
  if (isInjected) {
    return isLinkRef(mergedRef) && !isLinkRef(preRef)
  }
  return wasMergedInjected && isFileRef(mergedRef) && isLinkRef(preRef)
}

function isFileRef (ref: string): boolean {
  if (ref.startsWith('file:')) return true
  const parenIndex = ref.indexOf('(')
  const prefix = parenIndex === -1 ? ref : ref.slice(0, parenIndex)
  return prefix.indexOf('@file:', 1) !== -1
}

function pruneDependencyFields (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot | undefined,
  declaredDepNames: Record<DependenciesField, Set<string>>
): boolean {
  let pruned = false
  for (const depField of DEPENDENCIES_FIELDS) {
    if (pruneSingleDependencyField({
      importer,
      depField,
      preMergeDeps: preMergeImporter?.[depField],
      declaredNames: declaredDepNames[depField],
    })) {
      pruned = true
    }
  }
  return pruned
}

function pruneSingleDependencyField (opts: {
  importer: ProjectSnapshot
  depField: DependenciesField
  preMergeDeps: Record<string, string> | undefined
  declaredNames: Set<string>
}): boolean {
  const { importer, depField, preMergeDeps, declaredNames } = opts
  const deps = importer[depField]
  if (deps == null) return false
  let pruned = false
  for (const depName of Object.keys(deps)) {
    if (!declaredNames.has(depName) && (preMergeDeps == null || !Object.hasOwn(preMergeDeps, depName))) {
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
    if (!isDeclared && (preMergeImporter == null || !Object.hasOwn(preMergeImporter.specifiers, depName))) {
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
