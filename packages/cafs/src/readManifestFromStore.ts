import type { DeferredManifestPromise, PackageFileInfo } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { getFilePathByModeInCafs } from './getFilePathInCafs'
import { parseJsonBuffer } from './parseJson'

export async function readManifestFromStore (
  cafsDir: string,
  pkgIndex: Record<string, PackageFileInfo>,
  deferredManifest?: DeferredManifestPromise
) {
  const pkg = pkgIndex['package.json']

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
