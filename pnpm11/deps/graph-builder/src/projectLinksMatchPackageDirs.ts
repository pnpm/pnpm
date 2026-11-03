import { promises as fs } from 'node:fs'

import { refToRelative } from '@pnpm/deps.path'
import { isError } from '@pnpm/error'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'
import pLimit from 'p-limit'

/** Missing project links may use the root link when direct dependencies are deduplicated. */
export async function projectLinksMatchPackageDirs (
  projects: Array<{ id: ProjectId, modulesDir: string }>,
  { lockfile, packageDirs }: { lockfile: LockfileObject, packageDirs: ReadonlyMap<DepPath, string> }
): Promise<boolean> {
  const limit = pLimit(8)
  const rootModulesDir = projects.find(({ id }) => id === '.')?.modulesDir
  const links = projects.flatMap(({ id, modulesDir }) => {
    const importer = lockfile.importers[id]
    if (!importer) return []
    return Object.entries({ ...importer.dependencies, ...importer.devDependencies, ...importer.optionalDependencies })
      .map(([alias, ref]) => ({ alias, ref, modulesDir }))
  })
  const checks = links.map(({ alias, ref, modulesDir }) => limit(async () => {
    const depPath = refToRelative(ref, alias)
    const expected = depPath && packageDirs.get(depPath)
    if (!expected) return true
    return isSameRealPath(safeJoinModulesDir(modulesDir, alias), expected, rootModulesDir && safeJoinModulesDir(rootModulesDir, alias))
  }))
  return (await Promise.all(checks)).every(Boolean)
}

async function isSameRealPath (actualPath: string, expectedPath: string, rootPath?: string): Promise<boolean> {
  try {
    const [actualDir, expectedDir] = await Promise.all([
      resolveProjectLink(actualPath, rootPath),
      fs.realpath(expectedPath),
    ])
    return actualDir === expectedDir
  } catch (error: unknown) {
    if (isError(error) && 'code' in error && error.code === 'ENOENT') return false
    throw error
  }
}

async function resolveProjectLink (actualPath: string, rootPath?: string): Promise<string> {
  try {
    await fs.lstat(actualPath)
  } catch (error: unknown) {
    if (rootPath && isError(error) && 'code' in error && error.code === 'ENOENT') return fs.realpath(rootPath)
    throw error
  }
  return fs.realpath(actualPath)
}
