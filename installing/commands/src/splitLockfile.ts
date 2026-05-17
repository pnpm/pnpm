import { promises as fs } from 'node:fs'
import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import { filterLockfileByImporters } from '@pnpm/lockfile.filtering'
import {
  getLockfileImporterId,
  readWantedLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectId, ProjectRootDir } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'

const SPLIT_SENTINEL = '.pnpm-split-in-progress'

function sentinelPath (workspaceDir: string): string {
  return path.join(workspaceDir, SPLIT_SENTINEL)
}

/**
 * If a previous split-mode install crashed between merge and split,
 * a stale unified lockfile may remain at the workspace root.
 * Detect this via the sentinel file and clean up.
 */
export async function recoverFromPartialSplit (
  workspaceDir: string
): Promise<void> {
  try {
    await fs.access(sentinelPath(workspaceDir))
  } catch {
    return // no sentinel — nothing to recover
  }
  // Previous run crashed mid-split. Remove the stale unified lockfile.
  await rimraf(path.join(workspaceDir, WANTED_LOCKFILE))
  await rimraf(sentinelPath(workspaceDir))
}

/**
 * Read per-package lockfiles from each project directory,
 * merge them into a single unified lockfile, and write it
 * to the workspace root so that `mutateModules()` can read it normally.
 *
 * Writes a sentinel file before the merge so that partial failures
 * can be detected and cleaned up on the next run.
 */
export async function mergePerPackageLockfiles (
  workspaceDir: string,
  projectDirs: ProjectRootDir[]
): Promise<void> {
  // Write sentinel before creating the temporary unified lockfile
  await fs.writeFile(sentinelPath(workspaceDir), `pid=${process.pid}\n`)

  // Read all per-package lockfiles in parallel; merge sequentially below.
  const lockfiles = await Promise.all(projectDirs.map(async (projectDir) => ({
    projectDir,
    lockfile: await readWantedLockfile(projectDir, { ignoreIncompatible: true }),
  })))

  let merged: LockfileObject | null = null

  for (const { projectDir, lockfile } of lockfiles) {
    if (lockfile == null) continue

    const importerId = getLockfileImporterId(workspaceDir, projectDir)

    // Remap importers: "." → relative path from workspace root (e.g., "packages/foo")
    const remappedImporters: LockfileObject['importers'] = {}
    for (const [id, snapshot] of Object.entries(lockfile.importers)) {
      const newId = id === '.' ? importerId : `${importerId}/${id}` as ProjectId
      remappedImporters[newId] = snapshot
    }

    if (merged == null) {
      merged = {
        ...lockfile,
        importers: remappedImporters,
      }
    } else {
      Object.assign(merged.importers, remappedImporters)
      if (lockfile.packages) {
        if (!merged.packages) {
          merged.packages = {}
        }
        Object.assign(merged.packages, lockfile.packages)
      }
      if (lockfile.time) {
        if (!merged.time) {
          merged.time = {}
        }
        Object.assign(merged.time, lockfile.time)
      }
      if (lockfile.ignoredOptionalDependencies) {
        merged.ignoredOptionalDependencies = [...new Set([
          ...merged.ignoredOptionalDependencies ?? [],
          ...lockfile.ignoredOptionalDependencies,
        ])]
      }
    }
  }

  if (merged != null) {
    await writeWantedLockfile(workspaceDir, merged)
  }
}

/**
 * Read the unified lockfile from the workspace root, split it into
 * per-package lockfiles, write each to its package directory,
 * and remove the unified lockfile from the workspace root.
 */
export async function splitUnifiedLockfile (
  workspaceDir: string,
  projectDirs: ProjectRootDir[]
): Promise<void> {
  const lockfile = await readWantedLockfile(workspaceDir, {
    ignoreIncompatible: false,
  })
  if (lockfile == null) return

  const importerIds = projectDirs.map(
    (dir) => getLockfileImporterId(workspaceDir, dir)
  )
  const hasRootImporter = importerIds.includes('.' as ProjectId)

  await Promise.all(importerIds.map(async (importerId, i) => {
    const projectDir = projectDirs[i]

    // Check this importer actually exists in the lockfile
    if (!lockfile.importers[importerId]) return

    // Filter the lockfile to only include this importer's deps
    const filtered = filterLockfileByImporters(
      lockfile,
      [importerId],
      {
        include: {
          dependencies: true,
          devDependencies: true,
          optionalDependencies: true,
        },
        skipped: new Set(),
        failOnMissingDependencies: false,
      }
    )

    // Remap importerId from "packages/foo" → "."
    const perPkgLockfile: LockfileObject = {
      ...filtered,
      importers: {},
    }
    for (const [id, snapshot] of Object.entries(filtered.importers)) {
      if (id === importerId) {
        perPkgLockfile.importers['.' as ProjectId] = snapshot
      } else if (id.startsWith(`${importerId}/`)) {
        const newId = id.slice(importerId.length + 1) as ProjectId
        perPkgLockfile.importers[newId] = snapshot
      }
      // Drop importers that don't belong to this package
    }

    // Prune unreachable packages
    const pruned = pruneSharedLockfile(perPkgLockfile)
    await writeWantedLockfile(projectDir, pruned)
  }))

  // Remove the unified lockfile from workspace root, unless
  // the root itself is a project (in which case its per-package
  // lockfile was just written there).
  if (!hasRootImporter) {
    await rimraf(path.join(workspaceDir, WANTED_LOCKFILE))
  }

  // Split succeeded — remove the sentinel
  await rimraf(sentinelPath(workspaceDir))
}
