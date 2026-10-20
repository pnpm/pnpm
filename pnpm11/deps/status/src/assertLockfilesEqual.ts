import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject, PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.fs'
import type { DepPath, IncludedDependencies, ProjectId } from '@pnpm/types'
import { equals } from 'ramda'

export interface AssertLockfilesEqualOptions {
  wantedLockfileDir: string
  /**
   * The current lockfile was written by a filtered install, which records
   * only the packages of the projects it selected.
   */
  filteredInstall?: boolean
  importersToCheck?: ProjectId[]
  include?: IncludedDependencies
}

export function assertLockfilesEqual (currentLockfile: LockfileObject | null, wantedLockfile: LockfileObject, opts: AssertLockfilesEqualOptions): void {
  if (!currentLockfile) {
    // make sure that no importer of wantedLockfile has any dependency
    for (const [name, snapshot] of Object.entries(wantedLockfile.importers)) {
      if (!equals(snapshot.specifiers, {})) {
        throw new PnpmError('RUN_CHECK_DEPS_NO_DEPS', `Project ${name} requires dependencies but none was installed.`, {
          hint: 'Run `pnpm install` to install dependencies',
        })
      }
    }
  } else if (
    !equals(currentLockfile, wantedLockfile) &&
    !(opts.filteredInstall === true && filteredInstallLockfilesMatch(currentLockfile, wantedLockfile, opts))
  ) {
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_DEPS', `The installed dependencies in the modules directory is not up-to-date with the lockfile in ${opts.wantedLockfileDir}.`, {
      hint: 'Run `pnpm install` to update dependencies.',
    })
  }
}

function filteredInstallLockfilesMatch (
  currentLockfile: LockfileObject,
  wantedLockfile: LockfileObject,
  opts: AssertLockfilesEqualOptions
): boolean {
  return opts.importersToCheck != null && opts.include != null && Object.keys(wantedLockfile.importers).length > 0
    ? importerClosuresMatch(wantedLockfile, currentLockfile, opts.importersToCheck, opts.include)
    : recordsSubsetOfPackages(currentLockfile, wantedLockfile)
}

/**
 * Whether `currentLockfile` records what `wantedLockfile` materializes for
 * `initialImporterIds` in `include`.
 */
export function importerClosuresMatch (
  wantedLockfile: LockfileObject,
  currentLockfile: LockfileObject | null,
  initialImporterIds: ProjectId[],
  include: IncludedDependencies
): boolean {
  if (currentLockfile == null) {
    return initialImporterIds.every((id) => equals(wantedLockfile.importers[id]?.specifiers ?? {}, {}))
  }
  if (initialImporterIds.some((id) => currentLockfile.importers[id] == null)) {
    return false
  }
  const allGroups: IncludedDependencies = {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  return equals(
    collectImporterClosure(wantedLockfile, initialImporterIds, include),
    collectImporterClosure(currentLockfile, initialImporterIds, allGroups)
  )
}

interface ClosureState {
  lockfile: LockfileObject
  include: IncludedDependencies
  importers: Record<ProjectId, ProjectSnapshot>
  packages: PackageSnapshots
  importerQueue: ProjectId[]
  depQueue: DepPath[]
}

function collectImporterClosure (
  lockfile: LockfileObject,
  initialImporterIds: ProjectId[],
  include: IncludedDependencies
): { importers: Record<ProjectId, ProjectSnapshot>, packages: PackageSnapshots } {
  const state: ClosureState = {
    lockfile,
    include,
    importers: {} as Record<ProjectId, ProjectSnapshot>,
    packages: {} as PackageSnapshots,
    importerQueue: [...initialImporterIds],
    depQueue: [],
  }
  while (state.importerQueue.length > 0 || state.depQueue.length > 0) {
    drainImporterQueue(state)
    drainDepQueue(state)
  }
  return { importers: state.importers, packages: state.packages }
}

function drainImporterQueue (state: ClosureState): void {
  while (state.importerQueue.length > 0) {
    const importerId = state.importerQueue.pop()!
    const importer = state.lockfile.importers[importerId]
    if (state.importers[importerId] != null || importer == null) continue
    const filtered = filterImporterByGroups(importer, state.include)
    state.importers[importerId] = filtered
    enqueueRefs(state, importerId, filtered.dependencies)
    enqueueRefs(state, importerId, filtered.devDependencies)
    enqueueRefs(state, importerId, filtered.optionalDependencies)
  }
}

function drainDepQueue (state: ClosureState): void {
  while (state.depQueue.length > 0) {
    const depPath = state.depQueue.pop()!
    const pkg = state.lockfile.packages?.[depPath]
    if (state.packages[depPath] != null || pkg == null) continue
    state.packages[depPath] = pkg
    enqueueRefs(state, undefined, pkg.dependencies)
    if (state.include.optionalDependencies) {
      enqueueRefs(state, undefined, pkg.optionalDependencies)
    }
  }
}

function enqueueRefs (state: ClosureState, sourceImporterId: ProjectId | undefined, refs: Record<string, string> | undefined): void {
  for (const [pkgName, ref] of Object.entries(refs ?? {})) {
    if (ref.startsWith('link:')) {
      const targetId = resolveLinkedImporterId(sourceImporterId, ref.slice(5))
      if (state.lockfile.importers[targetId] != null && state.importers[targetId] == null) {
        state.importerQueue.push(targetId)
      }
      continue
    }
    const depPath = dp.refToRelative(ref, pkgName)
    if (depPath != null && state.packages[depPath] == null && state.lockfile.packages?.[depPath] != null) {
      state.depQueue.push(depPath)
    }
  }
}

function resolveLinkedImporterId (sourceImporterId: ProjectId | undefined, linkedPath: string): ProjectId {
  return (sourceImporterId == null || sourceImporterId === '.'
    ? path.posix.normalize(linkedPath)
    : path.posix.join(sourceImporterId, linkedPath)) as ProjectId
}

function filterImporterByGroups (importer: ProjectSnapshot, include: IncludedDependencies): ProjectSnapshot {
  const copy = { ...importer, specifiers: { ...importer.specifiers } }
  const omitGroup = (field: 'dependencies' | 'devDependencies' | 'optionalDependencies'): void => {
    for (const depName of Object.keys(copy[field] ?? {})) {
      delete copy.specifiers[depName]
    }
    delete copy[field]
  }
  if (!include.dependencies) {
    omitGroup('dependencies')
    omitGroup('optionalDependencies')
  } else if (!include.optionalDependencies) {
    omitGroup('optionalDependencies')
  }
  if (!include.devDependencies) {
    omitGroup('devDependencies')
  }
  return copy
}

/**
 * Whether `currentLockfile` equals `wantedLockfile` except for the packages
 * it leaves out. With the importers and every recorded package unchanged,
 * the packages any installed project reaches are unchanged too.
 */
function recordsSubsetOfPackages (currentLockfile: LockfileObject, wantedLockfile: LockfileObject): boolean {
  const { packages: currentPackages, ...currentRest } = currentLockfile
  const { packages: wantedPackages, ...wantedRest } = wantedLockfile
  return equals(currentRest, wantedRest) &&
    Object.entries(currentPackages ?? {}).every(([depPath, pkg]) =>
      equals(pkg, wantedPackages?.[depPath as keyof typeof wantedPackages]))
}
