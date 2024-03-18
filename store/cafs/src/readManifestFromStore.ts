import gfs from '@pnpm/graceful-fs'
import type { PackageFilesIndex } from './checkPkgFilesIntegrity'
import { getFilePathByModeInCafs } from './getFilePathInCafs'
import { parseJsonBufferSync } from './parseJson'
import type { DependencyManifest } from '@pnpm/types'

export function readManifestFromStore(
  cafsDir: string,
  pkgIndex: PackageFilesIndex
): DependencyManifest | undefined {
  const pkg = pkgIndex.files['package.json']
  if (pkg) {
    const fileName = getFilePathByModeInCafs(cafsDir, pkg.integrity, pkg.mode)
    return parseJsonBufferSync(gfs.readFileSync(fileName))
  }
  return undefined
}
