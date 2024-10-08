import fs from 'fs'
import path from 'path'
import { getCacheFilePath } from './cacheFile'
import { createPackagesList } from './createPackagesList'
import { type ProjectsList } from './types'

export interface UpdatePackagesListOptions {
  allProjects: ProjectsList
  cacheDir: string
  workspaceDir: string
}

export async function updatePackagesList (opts: UpdatePackagesListOptions): Promise<void> {
  const packagesList = createPackagesList(opts)
  const packagesListJSON = JSON.stringify(packagesList, undefined, 2) + '\n'
  const cacheFile = getCacheFilePath(opts)
  await fs.promises.mkdir(path.dirname(cacheFile), { recursive: true })
  await fs.promises.writeFile(cacheFile, packagesListJSON)
}
