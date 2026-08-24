import path from 'node:path'

import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  createLockfileObject,
  existsNonEmptyWantedLockfile,
  getGitBranchLockfileNames,
  isEmptyLockfile,
  type LockfileObject,
  type ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  readWantedLockfileAndAutofixConflicts,
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
    internalPnpmDir: string
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
}> {
  const wantedLockfileVersion = LOCKFILE_VERSION
  // On CI, avoid breaking builds due to incompatible lockfiles by default.
  // Ignore incompatible lockfiles only for non-frozen CI installs or when `force` is set;
  // in frozen-lockfile mode, incompatible lockfiles should still fail.
  const lockfileOpts = {
    ignoreIncompatible: opts.force || (opts.ci === true && !opts.frozenLockfile),
    wantedVersions: [LOCKFILE_VERSION],
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  }
  const fileReads = [] as Array<Promise<LockfileObject | undefined | null>>
  let lockfileHadConflicts: boolean = false
  let wantedLockfileFileExists = false
  if (opts.useLockfile) {
    wantedLockfileFileExists = await existsNonEmptyWantedLockfile(opts.lockfileDir, lockfileOpts)
    if (!opts.frozenLockfile) {
      fileReads.push(
        (async () => {
          try {
            const { lockfile, hadConflicts } = await readWantedLockfileAndAutofixConflicts(opts.lockfileDir, lockfileOpts)
            lockfileHadConflicts = hadConflicts
            return lockfile
          } catch (err: any) { // eslint-disable-line
            logger.warn({
              message: `Ignoring broken lockfile at ${opts.lockfileDir}: ${err.message as string}`,
              prefix: opts.lockfileDir,
            })
            return undefined
          }
        })()
      )
    } else {
      fileReads.push(readWantedLockfile(opts.lockfileDir, lockfileOpts))
    }
  } else {
    if (await existsNonEmptyWantedLockfile(opts.lockfileDir, lockfileOpts)) {
      logger.warn({
        message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a lockfile`,
        prefix: opts.lockfileDir,
      })
    }
    fileReads.push(Promise.resolve(undefined))
  }
  fileReads.push(
    (async () => {
      try {
        return await readCurrentLockfile(opts.internalPnpmDir, lockfileOpts)
      } catch (err: any) { // eslint-disable-line
        logger.warn({
          message: `Ignoring broken lockfile at ${opts.internalPnpmDir}: ${err.message as string}`,
          prefix: opts.lockfileDir,
        })
        return undefined
      }
    })()
  )
  const files = await Promise.all<LockfileObject | null | undefined>(fileReads)
  if (opts.frozenLockfile && wantedLockfileFileExists && files[0] == null) {
    throw new PnpmError('BROKEN_LOCKFILE', `The lockfile at "${path.join(opts.lockfileDir, WANTED_LOCKFILE)}" is broken: it is empty`)
  }
  const sopts = {
    autoInstallPeers: opts.autoInstallPeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    lockfileVersion: wantedLockfileVersion,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
  }
  const importerIds = opts.projects.map((importer) => importer.id)
  const currentLockfile = files[1] ?? createLockfileObject(importerIds, sopts)
  for (const importerId of importerIds) {
    if (!currentLockfile.importers[importerId]) {
      currentLockfile.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  const existsWantedLockfile = files[0] != null
  const existsCurrentLockfile = files[1] != null
  let wantedLockfile = files[0] ??
    (currentLockfile && clone(currentLockfile)) ??
    createLockfileObject(importerIds, sopts)
  // Cloning the current lockfile means the disk copy of the wanted lockfile is
  // stale, so flag it for rewriting after the install completes.
  let wantedLockfileIsModified = !existsWantedLockfile && existsCurrentLockfile
  for (const importerId of importerIds) {
    if (!wantedLockfile.importers[importerId]) {
      wantedLockfileIsModified = true
      wantedLockfile.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  // The merge takes the union of the two lockfiles' keys, so it can only add:
  // a dependency the manifests no longer declare is reinstated by it, and the
  // manifests are the only record that it is gone. Gated on a merge having
  // actually run, so that an ordinary lockfile the manifests have outgrown is
  // still reported by the frozen check rather than quietly repaired here.
  if (opts.mergeGitBranchLockfiles && existsWantedLockfile && (await getGitBranchLockfileNames(opts.lockfileDir)).length > 0) {
    for (const project of opts.projects) {
      pruneUndeclaredDependencies(wantedLockfile.importers[project.id], project.manifest, opts.autoInstallPeers)
    }
    wantedLockfile = pruneSharedLockfile(wantedLockfile)
  }
  return {
    currentLockfile,
    currentLockfileIsUpToDate: equals(currentLockfile, wantedLockfile),
    existsCurrentLockfile,
    existsWantedLockfile,
    existsNonEmptyWantedLockfile: existsWantedLockfile && !isEmptyLockfile(wantedLockfile),
    wantedLockfile,
    wantedLockfileIsModified,
    lockfileHadConflicts,
  }
}

function pruneUndeclaredDependencies (
  importer: ProjectSnapshot,
  manifest: ProjectManifest,
  autoInstallPeers: boolean
): void {
  const declaredDepNames = declaredDepNamesByField(manifest, autoInstallPeers)
  for (const depField of DEPENDENCIES_FIELDS) {
    const deps = importer[depField]
    if (deps == null) continue
    for (const depName of Object.keys(deps)) {
      if (!declaredDepNames[depField].has(depName)) {
        delete deps[depName]
      }
    }
    if (Object.keys(deps).length === 0) {
      delete importer[depField]
    }
  }
  for (const depName of Object.keys(importer.specifiers)) {
    if (DEPENDENCIES_FIELDS.every((depField) => !declaredDepNames[depField].has(depName))) {
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
    // Only a peer that no other field declares is auto-installed; one that is
    // also declared elsewhere stays under the field that declares it.
    for (const depName of Object.keys(manifest.peerDependencies ?? {})) {
      if (!optionalDependencies.has(depName) && !devDependencies.has(depName)) {
        dependencies.add(depName)
      }
    }
  }
  return { dependencies, devDependencies, optionalDependencies }
}
