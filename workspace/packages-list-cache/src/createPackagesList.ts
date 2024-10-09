import fs from 'fs'
import path from 'path'
import { type ProjectRootDir } from '@pnpm/types'
import { type PackagesList, type ProjectsList, type TimestampMap } from './types'

export interface CreatePackagesListOptions {
  allProjects: ProjectsList
  workspaceDir: string
}

export async function createPackagesList (opts: CreatePackagesListOptions): Promise<PackagesList> {
  const entries = await Promise.all(opts.allProjects.map(async project => {
    const projectManifestPath = path.join(project.rootDir, 'package.json')
    const stats = await fs.promises.stat(projectManifestPath)
    return [project.rootDir, {
      'package.json': stats.mtime.valueOf(),
    }] as [ProjectRootDir, TimestampMap]
  }))
  return {
    modificationTimestamps: Object.fromEntries(entries),
    workspaceDir: opts.workspaceDir,
  }
}
