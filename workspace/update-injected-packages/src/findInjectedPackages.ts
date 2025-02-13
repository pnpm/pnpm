import path from 'path'
import * as dp from '@pnpm/dependency-path'
import { type DirectoryResolution, type LockfileObject } from '@pnpm/lockfile.fs'
import { type DepPath } from '@pnpm/types'

export interface FindInjectedPackagesOptions {
  lockfile: Pick<LockfileObject, 'packages'>
  pkgName: string
  pkgRootDir: string
  workspaceDir: string
}

export interface InjectedPackageInfo {
  depPath: DepPath
  parsedDepPath: dp.DependencyPath
  resolution: DirectoryResolution
  resolvedDirectory: string
}

export function * findInjectedPackages ({
  lockfile,
  pkgName,
  pkgRootDir,
  workspaceDir,
}: FindInjectedPackagesOptions): Generator<InjectedPackageInfo> {
  for (const _depPath in lockfile.packages) {
    const depPath = _depPath as DepPath
    const { resolution } = lockfile.packages[depPath]
    if (!('type' in resolution && resolution.type === 'directory')) continue
    const parsedDepPath = dp.parse(depPath)
    if (parsedDepPath.name !== pkgName) continue
    const resolvedDirectory = path.join(workspaceDir, resolution.directory)
    if (resolvedDirectory !== pkgRootDir) continue
    yield {
      depPath,
      parsedDepPath,
      resolution,
      resolvedDirectory,
    }
  }
}
