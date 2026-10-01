import path from 'node:path'

import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  checkPatchedDepPaths,
  createLockfileObject,
  existsNonEmptyWantedLockfile,
  isEmptyLockfile,
  type LockfileObject,
  type PatchedDepPathsStatus,
  type ProjectSnapshot,
  readCurrentLockfile,
  readEnvLockfile,
  readWantedLockfileWithMergeInfo,
} from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import { logger } from '@pnpm/logger'
import { DEPENDENCIES_FIELDS, type DependenciesField, type ProjectId, type ProjectManifest, type ProjectRootDir } from '@pnpm/types'
import { clone, equals } from 'ramda'

export interface PnpmContext {
  currentLockfile: LockfileObject
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  wantedLockfile: LockfileObject
}

interface LockfileFsOptions {
  ignoreIncompatible: boolean
  wantedVersions: string[]
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

interface CreateLockfileOpts {
  autoInstallPeers: boolean
  excludeLinksFromLockfile: boolean
  lockfileVersion: string
  peersSuffixMaxLength: number
}

export async function readLockfiles (
  opts: {
    autoInstallPeers: boolean
    excludeLinksFromLockfile: boolean
    peersSuffixMaxLength: number
    ci?: boolean
    force: boolean
    frozenLockfile: boolean
    projects: Array<{
      id: ProjectId
      manifest: ProjectManifest
      rootDir: ProjectRootDir
    }>
    lockfileDir: string
    registry: string
    useLockfile: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
    installStateDir: string
  }
): Promise<{
  currentLockfile: LockfileObject
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  wantedLockfile: LockfileObject
  wantedLockfileIsModified: boolean
  lockfileHadConflicts: boolean
  patchedDepPathsStatus: PatchedDepPathsStatus
}> {
  const lockfileOpts: LockfileFsOptions = {
    ignoreIncompatible: opts.force || (opts.ci === true && !opts.frozenLockfile),
    wantedVersions: [LOCKFILE_VERSION],
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  }

  const wantedResult = await loadWantedLockfile(opts, lockfileOpts)
  const currentLockfileRaw = await loadCurrentLockfile(opts.installStateDir, opts.lockfileDir, lockfileOpts)
  const importerIds = opts.projects.map((importer) => importer.id)
  const sopts: CreateLockfileOpts = {
    autoInstallPeers: opts.autoInstallPeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    lockfileVersion: LOCKFILE_VERSION,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
  }

  const currentLockfile = initLockfileImporters(currentLockfileRaw ?? createLockfileObject(importerIds, sopts), importerIds)
  const existsCurrentLockfile = currentLockfileRaw != null
  const existsWantedLockfile = wantedResult.lockfile != null

  const { wantedLockfile, wantedLockfileIsModified } = initWantedLockfile({
    importerIds,
    sopts,
    rawWantedLockfile: wantedResult.lockfile,
    currentLockfile,
    existsWantedLockfile,
    existsCurrentLockfile,
    preMergeImporters: wantedResult.preMergeImporters,
    projects: opts.projects,
    autoInstallPeers: opts.autoInstallPeers,
  })

  return assembleLockfilesResult({
    currentLockfile,
    wantedLockfile,
    existsCurrentLockfile,
    existsWantedLockfile,
    wantedLockfileIsModified,
    wantedResult,
    frozenLockfile: opts.frozenLockfile,
  })
}

function assembleLockfilesResult (params: {
  currentLockfile: LockfileObject
  wantedLockfile: LockfileObject
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  wantedLockfileIsModified: boolean
  wantedResult: LoadedWantedResult
  frozenLockfile: boolean
}): {
  currentLockfile: LockfileObject
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  wantedLockfile: LockfileObject
  wantedLockfileIsModified: boolean
  lockfileHadConflicts: boolean
  patchedDepPathsStatus: PatchedDepPathsStatus
} {
  const { currentLockfile, wantedLockfile, existsCurrentLockfile, existsWantedLockfile, wantedLockfileIsModified, wantedResult, frozenLockfile } = params
  const patchedDepPathsStatus = resolvePatchedDepPathsStatus({
    frozenLockfile,
    existsWantedLockfile,
    existsCurrentLockfile,
    lockfileHadConflicts: wantedResult.hadConflicts,
    shouldCheckPatchedDepPaths: wantedResult.shouldCheckPatchedDepPaths,
    wantedLockfile,
    initialStatus: wantedResult.initialPatchedStatus,
  })

  return {
    currentLockfile,
    currentLockfileIsUpToDate: equals(currentLockfile, wantedLockfile),
    existsCurrentLockfile,
    existsWantedLockfile,
    existsNonEmptyWantedLockfile: existsWantedLockfile && !isEmptyLockfile(wantedLockfile),
    wantedLockfile,
    wantedLockfileIsModified,
    lockfileHadConflicts: wantedResult.hadConflicts,
    patchedDepPathsStatus,
  }
}

interface LoadedWantedResult {
  lockfile: LockfileObject | undefined
  hadConflicts: boolean
  preMergeImporters?: LockfileObject['importers']
  shouldCheckPatchedDepPaths: boolean
  initialPatchedStatus: PatchedDepPathsStatus
}

async function loadWantedLockfile (
  opts: {
    useLockfile: boolean
    frozenLockfile: boolean
    lockfileDir: string
  },
  lockfileOpts: LockfileFsOptions
): Promise<LoadedWantedResult> {
  if (!opts.useLockfile) {
    if (await existsNonEmptyWantedLockfile(opts.lockfileDir, lockfileOpts)) {
      logger.warn({
        message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a lockfile`,
        prefix: opts.lockfileDir,
      })
    }
    return { lockfile: undefined, hadConflicts: false, shouldCheckPatchedDepPaths: false, initialPatchedStatus: 'up-to-date' }
  }

  const wantedFileExists = await existsNonEmptyWantedLockfile(opts.lockfileDir, lockfileOpts)
  const result = opts.frozenLockfile
    ? await readWantedFrozen(opts.lockfileDir, lockfileOpts)
    : await readWantedNonFrozen(opts.lockfileDir, lockfileOpts)

  if (opts.frozenLockfile && wantedFileExists && result.lockfile == null && !(await hasEnvDocument(opts.lockfileDir))) {
    throw new PnpmError('BROKEN_LOCKFILE', `The lockfile at "${path.join(opts.lockfileDir, WANTED_LOCKFILE)}" is broken: it is empty`)
  }
  return result
}

/**
 * A lockfile that records only the env document has no main document yet,
 * which is what pnpm writes for a project before its first install. An env
 * document that does not parse leaves the lockfile broken.
 */
async function hasEnvDocument (lockfileDir: string): Promise<boolean> {
  try {
    return await readEnvLockfile(lockfileDir) != null
  } catch {
    return false
  }
}

async function readWantedFrozen (lockfileDir: string, lockfileOpts: LockfileFsOptions): Promise<LoadedWantedResult> {
  const read = await readWantedLockfileWithMergeInfo(lockfileDir, lockfileOpts)
  return {
    lockfile: read.lockfile ?? undefined,
    hadConflicts: false,
    preMergeImporters: read.preMergeImporters,
    shouldCheckPatchedDepPaths: read.lockfile != null,
    initialPatchedStatus: 'up-to-date',
  }
}

async function readWantedNonFrozen (lockfileDir: string, lockfileOpts: LockfileFsOptions): Promise<LoadedWantedResult> {
  try {
    const read = await readWantedLockfileWithMergeInfo(lockfileDir, { ...lockfileOpts, autofixMergeConflicts: true })
    const hadConflicts = read.hadConflicts
    return {
      lockfile: read.lockfile ?? undefined,
      hadConflicts,
      preMergeImporters: read.preMergeImporters,
      shouldCheckPatchedDepPaths: !hadConflicts && read.lockfile != null,
      initialPatchedStatus: hadConflicts ? 'indeterminate' : 'up-to-date',
    }
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err)
    logger.warn({
      message: `Ignoring broken lockfile at ${lockfileDir}: ${message}`,
      prefix: lockfileDir,
    })
    return { lockfile: undefined, hadConflicts: false, shouldCheckPatchedDepPaths: false, initialPatchedStatus: 'up-to-date' }
  }
}

async function loadCurrentLockfile (installStateDir: string, lockfileDir: string, lockfileOpts: LockfileFsOptions): Promise<LockfileObject | undefined> {
  try {
    const lockfile = await readCurrentLockfile(installStateDir, lockfileOpts)
    return lockfile ?? undefined
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err)
    logger.warn({
      message: `Ignoring broken lockfile at ${installStateDir}: ${message}`,
      prefix: lockfileDir,
    })
    return undefined
  }
}

function initLockfileImporters (lockfile: LockfileObject, importerIds: ProjectId[]): LockfileObject {
  for (const importerId of importerIds) {
    lockfile.importers[importerId] = lockfile.importers[importerId] ?? { specifiers: {} }
  }
  return lockfile
}

function initWantedLockfile (opts: {
  importerIds: ProjectId[]
  sopts: CreateLockfileOpts
  rawWantedLockfile: LockfileObject | undefined
  currentLockfile: LockfileObject
  existsWantedLockfile: boolean
  existsCurrentLockfile: boolean
  preMergeImporters?: LockfileObject['importers']
  projects: Array<{ id: ProjectId, manifest: ProjectManifest }>
  autoInstallPeers: boolean
}): { wantedLockfile: LockfileObject, wantedLockfileIsModified: boolean } {
  const wantedLockfile = opts.rawWantedLockfile ??
    (opts.currentLockfile && clone(opts.currentLockfile)) ??
    createLockfileObject(opts.importerIds, opts.sopts)

  let wantedLockfileIsModified = !opts.existsWantedLockfile && opts.existsCurrentLockfile
  for (const importerId of opts.importerIds) {
    if (!wantedLockfile.importers[importerId]) {
      wantedLockfileIsModified = true
      wantedLockfile.importers[importerId] = { specifiers: {} }
    }
  }

  const prunedLockfile = maybePruneMergedImporters(wantedLockfile, opts.preMergeImporters, opts.projects, opts.autoInstallPeers)
  return { wantedLockfile: prunedLockfile, wantedLockfileIsModified }
}

function maybePruneMergedImporters (
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

function resolvePatchedDepPathsStatus (opts: {
  frozenLockfile: boolean
  existsWantedLockfile: boolean
  existsCurrentLockfile: boolean
  lockfileHadConflicts: boolean
  shouldCheckPatchedDepPaths: boolean
  wantedLockfile: LockfileObject
  initialStatus: PatchedDepPathsStatus
}): PatchedDepPathsStatus {
  const needsCheck = opts.shouldCheckPatchedDepPaths ||
    (!opts.frozenLockfile && !opts.existsWantedLockfile && opts.existsCurrentLockfile && !opts.lockfileHadConflicts)
  if (needsCheck) {
    return checkPatchedDepPaths(opts.wantedLockfile)
  }
  return opts.initialStatus
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
  return depsPruned
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
