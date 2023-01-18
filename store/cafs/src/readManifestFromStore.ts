import type { DeferredManifestPromise } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { PackageFilesIndex } from './checkPkgFilesIntegrity'
import { getFilePathByModeInCafs } from './getFilePathInCafs'
import { parseJsonBuffer } from './parseJson'

export async function readManifestFromStore (
  cafsDir: string,
  pkgIndex: PackageFilesIndex,
  deferredManifest?: DeferredManifestPromise
) {
  const pkg = pkgIndex.files['package.json']

  if (deferredManifest) {
    if (pkg) {
      const fileName = getFilePathByModeInCafs(cafsDir, pkg.integrity, pkg.mode)
      parseJsonBuffer(await gfs.readFile(fileName), deferredManifest)
    } else {
      deferredManifest.resolve(undefined)
    }
  }

  return true
}
