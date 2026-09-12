import path from 'node:path'

import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  createLockfileObject,
  existsNonEmptyWantedLockfile,
  isEmptyLockfile,
  type LockfileObject,
  type ProjectSnapshot,
  readCurrentLockfile,
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
  let preMergeImporters: LockfileObject['importers'] | undefined
  let wantedLockfileFileExists = false
  if (opts.useLockfile) {
    wantedLockfileFileExists = await existsNonEmptyWantedLockfile(opts.lockfileDir, lockfileOpts)
    if (!opts.frozenLockfile) {
      fileReads.push(
        (async () => {
          try {
            const read = await readWantedLockfileWithMergeInfo(opts.lockfileDir, { ...lockfileOpts, autofixMergeConflicts: true })
            lockfileHadConflicts = read.hadConflicts
            preMergeImporters = read.preMergeImporters
            return read.lockfile
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
      fileReads.push(
        (async () => {
          const read = await readWantedLockfileWithMergeInfo(opts.lockfileDir, lockfileOpts)
          preMergeImporters = read.preMergeImporters
          return read.lockfile
        })()
      )
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
  // The merge takes the union of the two lockfiles' keys, so a dependency the
  // manifests no longer declare comes back with it, and the manifests are the
  // only record that it is gone. Entries the read file already carried are
  // left alone: that drift is the frozen check's to report, not the merge's
  // to repair.
  if (preMergeImporters != null) {
    let prunedAnyImporter = false
    for (const project of opts.projects) {
      prunedAnyImporter = pruneMergedDependencies({
        importer: wantedLockfile.importers[project.id],
        preMergeImporter: preMergeImporters[project.id],
        manifest: project.manifest,
        autoInstallPeers: opts.autoInstallPeers,
      }) || prunedAnyImporter
    }
    if (prunedAnyImporter) {
      wantedLockfile = pruneSharedLockfile(wantedLockfile)
    }
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
  let pruned = false
  for (const depField of DEPENDENCIES_FIELDS) {
    const deps = importer[depField]
    if (deps == null) continue
    for (const depName of Object.keys(deps)) {
      if (!declaredDepNames[depField].has(depName) && preMergeImporter?.[depField]?.[depName] == null) {
        delete deps[depName]
        pruned = true
      }
    }
    if (Object.keys(deps).length === 0) {
      delete importer[depField]
    }
  }
  for (const depName of Object.keys(importer.specifiers)) {
    if (
      DEPENDENCIES_FIELDS.every((depField) => !declaredDepNames[depField].has(depName)) &&
      preMergeImporter?.specifiers?.[depName] == null
    ) {
      delete importer.specifiers[depName]
    }
  }
  return pruned
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
