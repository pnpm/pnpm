import fs from 'fs'
import path from 'path'
import { type Catalogs } from '@pnpm/catalogs.types'
import { logger } from '@pnpm/logger'
import { getCacheFilePath } from './cacheFile'
import { createPackagesList } from './createPackagesList'
import { type ProjectsList } from './types'

export interface UpdatePackagesListOptions {
  allProjects: ProjectsList
  catalogs?: Catalogs
  filtered: boolean
  lastValidatedTimestamp: number
  workspaceDir: string
}

export async function updatePackagesList (opts: UpdatePackagesListOptions): Promise<void> {
  logger.debug({ msg: 'updating packages list' })
  const packagesList = createPackagesList(opts)
  const packagesListJSON = JSON.stringify(packagesList, undefined, 2) + '\n'
  const cacheFile = getCacheFilePath(opts.workspaceDir)
  await fs.promises.mkdir(path.dirname(cacheFile), { recursive: true })
  await fs.promises.writeFile(cacheFile, packagesListJSON)
}
