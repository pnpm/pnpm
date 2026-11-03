import { promises as fs } from 'node:fs'

import { refToRelative } from '@pnpm/deps.path'
import { isError } from '@pnpm/error'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

/** Whether each installed project link points at its expected package directory. */
export async function projectLinksMatchPackageDirs (
  projects: Array<{ id: ProjectId, modulesDir: string }>,
  { lockfile, packageDirs }: { lockfile: LockfileObject, packageDirs: ReadonlyMap<DepPath, string> }
): Promise<boolean> {
  const checks = projects.flatMap(({ id, modulesDir }) => {
    const importer = lockfile.importers[id]
    if (!importer) return []
    return Object.entries({ ...importer.dependencies, ...importer.devDependencies, ...importer.optionalDependencies })
      .map(async ([alias, ref]) => {
        const depPath = refToRelative(ref, alias)
        const expected = depPath && packageDirs.get(depPath)
        if (!expected) return true
        return isSameRealPath(safeJoinModulesDir(modulesDir, alias), expected)
      })
  })
  return (await Promise.all(checks)).every(Boolean)
}

/** Whether both paths resolve to the same directory. `false` when either is missing. */
async function isSameRealPath (actualPath: string, expectedPath: string): Promise<boolean> {
  try {
    const [actualDir, expectedDir] = await Promise.all([
      fs.realpath(actualPath),
      fs.realpath(expectedPath),
    ])
    return actualDir === expectedDir
  } catch (error: unknown) {
    if (isError(error) && 'code' in error && error.code === 'ENOENT') return false
    throw error
  }
}
