import { promises as fs } from 'node:fs'
import path from 'node:path'

import { removeBin, removeBinsOfDependency } from '@pnpm/bins.remover'
import { rootLogger } from '@pnpm/core-loggers'
import { isError } from '@pnpm/error'
import type { DependenciesField, ProjectRootDir } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'

export async function removeDirectDependency (
  dependency: {
    dependenciesField?: DependenciesField | undefined
    name: string
    /**
     * The wanted lockfile still records the dependency, so its removal is
     * reported only when there is an entry to remove.
     */
    stillWanted?: boolean
  },
  opts: {
    binsDir: string
    dryRun?: boolean
    modulesDir: string
    muteLogs?: boolean
    rootDir: ProjectRootDir
  }
): Promise<void> {
  const dependencyDir = path.join(opts.modulesDir, dependency.name)
  if (dependency.stillWanted && !await entryExists(dependencyDir)) return
  // The bins are found through the dependency's manifest, so the dependency
  // directory is removed only after they are.
  const uninstalledPkg = await removeBinsOfDependency(dependencyDir, opts)
  if (!opts.dryRun) {
    await removeBin(dependencyDir)
  }
  await removeIfEmpty(opts.binsDir)

  if (!opts.muteLogs) {
    rootLogger.debug({
      prefix: opts.rootDir,
      removed: {
        dependencyType: dependency.dependenciesField === 'devDependencies' && 'dev' ||
          dependency.dependenciesField === 'optionalDependencies' && 'optional' ||
          dependency.dependenciesField === 'dependencies' && 'prod' ||
          undefined,
        name: dependency.name,
        version: uninstalledPkg?.version,
      },
    })
  }
}

async function entryExists (entryPath: string): Promise<boolean> {
  try {
    await fs.lstat(entryPath)
    return true
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

export async function removeIfEmpty (dir: string): Promise<void> {
  if (await dirIsEmpty(dir)) {
    await rimraf(dir)
  }
}

async function dirIsEmpty (dir: string): Promise<boolean> {
  try {
    const fileNames = await fs.readdir(dir)
    return fileNames.length === 0
  } catch {
    return false
  }
}
