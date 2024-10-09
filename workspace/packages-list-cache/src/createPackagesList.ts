import fs from 'fs'
import path from 'path'
import util from 'util'
import { type Catalogs } from '@pnpm/catalogs.types'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { type ProjectRootDir } from '@pnpm/types'
import { type PackagesList, type ProjectInfo, type ProjectsList } from './types'

export interface CreatePackagesListOptions {
  allProjects: ProjectsList
  catalogs?: Catalogs
  workspaceDir: string
}

export async function createPackagesList (opts: CreatePackagesListOptions): Promise<PackagesList> {
  const entries = await Promise.all(opts.allProjects.map(async project => {
    const readAttempts = await Promise.all(MANIFEST_BASE_NAMES.map(async manifestBaseName => {
      const projectManifestPath = path.join(project.rootDir, manifestBaseName)
      let stats: fs.Stats
      try {
        stats = await fs.promises.stat(projectManifestPath)
      } catch (error) {
        if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
          return undefined
        }
        throw error
      }
      return [project.rootDir, {
        manifestBaseName,
        manifestModificationTimestamp: stats.mtime.valueOf(),
      }] as [ProjectRootDir, ProjectInfo]
    }))
    const entry = readAttempts.find(result => result !== undefined)
    if (!entry) {
      throw new Error(`Cannot find a manifest file in ${project.rootDir}`) // this is a programmer error, not a user error
    }
    return entry
  }))
  return {
    catalogs: opts.catalogs,
    projects: Object.fromEntries(entries),
    workspaceDir: opts.workspaceDir,
  }
}
