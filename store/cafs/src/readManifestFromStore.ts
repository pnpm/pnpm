import gfs from '@pnpm/graceful-fs'
import { type PackageManifest } from '@pnpm/types'
import { type PackageFilesIndex } from './checkPkgFilesIntegrity.js'
import { getFilePathByModeInCafs } from './getFilePathInCafs.js'
import { parseJsonBufferSync } from './parseJson.js'

export function readManifestFromStore (storeDir: string, pkgIndex: PackageFilesIndex): PackageManifest | undefined {
  // First, try to read from the cached manifest in the index file
  if (pkgIndex.manifest) {
    return pkgIndex.manifest as unknown as PackageManifest
  }
  // Fall back to reading from CAS for backward compatibility with old index files
  const pkg = pkgIndex.files.get('package.json')
  if (pkg) {
    const fileName = getFilePathByModeInCafs(storeDir, pkg.digest, pkg.mode)
    return parseJsonBufferSync(gfs.readFileSync(fileName)) as PackageManifest
  }
  return undefined
}
