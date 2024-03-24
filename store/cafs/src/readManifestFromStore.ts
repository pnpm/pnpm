import gfs from '@pnpm/graceful-fs'
import type { DependencyManifest, PackageFilesIndex } from '@pnpm/types'

import { parseJsonBufferSync } from './parseJson.js'
import { getFilePathByModeInCafs } from './getFilePathInCafs.js'

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
