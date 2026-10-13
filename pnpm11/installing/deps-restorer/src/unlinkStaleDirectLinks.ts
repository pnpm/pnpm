import { promises as fs } from 'node:fs'
import path from 'node:path'

import { removeBinsOfDependency } from '@pnpm/bins.remover'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'

import type { HeadlessContext } from './context.js'
import { isSymlink, unlinkIfSymlink } from './unlinkIfSymlink.js'

/**
 * Unlinks the direct dependencies of the selected projects that the wanted
 * lockfile no longer records at the same version. The hoisted linker's orphan
 * diff covers only the package directories it imported, so the `link:` and
 * workspace links written beside them need this pass. Real directories are
 * left to the orphan diff.
 */
export async function unlinkStaleDirectLinks (
  { currentLockfile, selectedProjects }: HeadlessContext,
  wantedLockfile: LockfileObject
): Promise<void> {
  if (currentLockfile == null) return
  await Promise.all(selectedProjects.map(async ({ binsDir, id, modulesDir }) => {
    const wantedImporter = wantedLockfile.importers[id]
    if (wantedImporter == null) return
    const wantedDeps = mergeDependencies(wantedImporter)
    const staleAliases = Object.entries(mergeDependencies(currentLockfile.importers[id]))
      .filter(([alias, ref]) => wantedDeps[alias] !== ref)
      .map(([alias]) => alias)
    await Promise.all(staleAliases.map(async (alias) => unlinkDirectLink(modulesDir, alias, binsDir)))
  }))
}

function mergeDependencies (snapshot: ProjectSnapshot | undefined): Record<string, string> {
  return Object.assign({}, ...DEPENDENCIES_FIELDS.map((depType) => snapshot?.[depType]))
}

async function unlinkDirectLink (modulesDir: string, alias: string, binsDir: string): Promise<void> {
  const link = safeJoinModulesDir(modulesDir, alias)
  // A symlinked `@scope` directory would redirect the unlink outside the modules directory.
  if (alias.startsWith('@') && !await isRealDir(path.dirname(link))) return
  if (!await isSymlink(link)) return
  // Unlinking through a symlinked `.bin` would delete files outside the project.
  if (await isRealDir(binsDir)) {
    await removeBinsOfDependency(link, { binsDir })
  }
  await unlinkIfSymlink(link)
}

async function isRealDir (dir: string): Promise<boolean> {
  const stats = await fs.lstat(dir).catch(() => undefined)
  return stats?.isDirectory() === true
}
