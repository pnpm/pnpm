import gfs from '@pnpm/graceful-fs'
import { type PackageManifest } from '@pnpm/types'
import { type PackageFilesIndex } from './checkPkgFilesIntegrity'
import { getFilePathByModeInCafs } from './getFilePathInCafs'
import { parseJsonBufferSync } from './parseJson'

export function readManifestFromStore (storeDir: string, pkgIndex: PackageFilesIndex): PackageManifest | undefined {
  const pkg = pkgIndex.files['package.json']
  if (pkg) {
    const fileName = getFilePathByModeInCafs(storeDir, pkg.integrity, pkg.mode)
    return parseJsonBufferSync(gfs.readFileSync(fileName)) as PackageManifest
  }
  return undefined
}
