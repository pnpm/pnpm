import { promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { findCommonPathAncestor, validateWorkspaceModulesDir } from '@pnpm/fs.symlink-dependency'
import type { HoistedWorkspaceProject } from '@pnpm/installing.linking.hoist'
import type { HoistedDependencies, ProjectId } from '@pnpm/types'

import { removeOrphanBins } from './linkHoistedModules.js'
import type { Project } from './types.js'

export function getHoistedWorkspacePackages (
  projects: Record<string, Project>,
  reservedAliases: string[] = []
): Record<ProjectId, HoistedWorkspaceProject> {
  const reserved = new Set(reservedAliases.map((alias) => alias.toLowerCase()))
  const hoistedWorkspacePackages = {} as Record<ProjectId, HoistedWorkspaceProject>
  for (const project of Object.values(projects)) {
    if (project.manifest.name && project.id !== '.' && !reserved.has(project.manifest.name.toLowerCase())) {
      hoistedWorkspacePackages[project.id] = {
        dir: project.rootDir,
        name: project.manifest.name,
      }
    }
  }
  return hoistedWorkspacePackages
}

export async function workspaceHoistPointsToProject (projectId: ProjectId, aliases: Record<string, 'private' | 'public'>, opts: {
  lockfileDir: string
  privateHoistedModulesDir: string
  publicHoistedModulesDir: string
}): Promise<boolean> {
  const projectDir = path.resolve(opts.lockfileDir, projectId)
  if (!projectDir.startsWith(path.resolve(opts.lockfileDir) + path.sep)) return false
  return (await Promise.all(Object.entries(aliases).map(async ([alias, kind]) => {
    const modulesDir = kind === 'public' ? opts.publicHoistedModulesDir : opts.privateHoistedModulesDir
    const trustedRoot = findCommonPathAncestor(opts.publicHoistedModulesDir, modulesDir) ?? path.parse(path.resolve(modulesDir)).root
    const destination = await validateWorkspaceModulesDir(modulesDir, alias, trustedRoot)
    try {
      const target = await fs.readlink(destination)
      return path.resolve(path.dirname(destination), target) === projectDir
    } catch (error: unknown) {
      if (isError(error) && 'code' in error && (error.code === 'ENOENT' || error.code === 'EINVAL')) return false
      throw error
    }
  }))).some(Boolean)
}

export async function removeBinsOfWorkspaceHoists (hoistedDependencies: HoistedDependencies, projectIds: Set<ProjectId>, modulesDir: string): Promise<void> {
  const aliases = Array.from(projectIds).flatMap((projectId) => Object.keys(hoistedDependencies[projectId] ?? {}))
  await Promise.all(aliases.map(async (alias) => removeBinsOfWorkspaceHoist(modulesDir, alias)))
}

async function removeBinsOfWorkspaceHoist (modulesDir: string, alias: string): Promise<void> {
  const link = await validateWorkspaceModulesDir(modulesDir, alias)
  let stats: Stats
  try {
    stats = await fs.lstat(link)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  if (stats.isSymbolicLink()) await removeOrphanBins(link)
}
