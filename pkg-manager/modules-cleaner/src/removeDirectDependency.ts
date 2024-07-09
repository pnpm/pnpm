import path from 'path'
import { promises as fs } from 'fs'
import { rootLogger } from '@pnpm/core-loggers'
import { removeBin, removeBinsOfDependency } from '@pnpm/remove-bins'
import { type DependenciesField, type ProjectRootDir } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'

export async function removeDirectDependency (
  dependency: {
    dependenciesField?: DependenciesField | undefined
    name: string
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
  const results = await Promise.all([
    removeBinsOfDependency(dependencyDir, opts),
    !opts.dryRun && removeBin(dependencyDir) as any, // eslint-disable-line @typescript-eslint/no-explicit-any
  ])
  await removeIfEmpty(opts.binsDir)

  const uninstalledPkg = results[0]
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
