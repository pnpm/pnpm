import { type DirectoryResolution, type LockfileObject } from '@pnpm/lockfile.fs'
import { type DepPath } from '@pnpm/types'

export interface InjectedPackageInfo {
  depPath: DepPath
  resolution: DirectoryResolution
}

export function * injectedPackages (lockfile: LockfileObject): Generator<InjectedPackageInfo> {
  for (const _depPath in lockfile.packages) {
    const depPath = _depPath as DepPath
    const { resolution } = lockfile.packages[depPath]
    if ('type' in resolution && resolution.type === 'directory') {
      yield { depPath, resolution }
    }
  }
}
