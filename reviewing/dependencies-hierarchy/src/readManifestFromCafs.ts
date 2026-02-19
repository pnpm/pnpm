import { readMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { loadJsonFileSync } from 'load-json-file'
import { getIndexFilePathInCafs, getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { type DependencyManifest } from '@pnpm/types'

/**
 * Attempts to read a package manifest from the content-addressable store (CAFS)
 * using its integrity hash. Returns `undefined` if the manifest cannot be read.
 */
export function readManifestFromCafs (storeDir: string, pkg: {
  integrity: string
  name: string
  version: string
}): DependencyManifest | undefined {
  try {
    const pkgId = `${pkg.name}@${pkg.version}`
    const indexPath = getIndexFilePathInCafs(storeDir, pkg.integrity, pkgId)
    const pkgIndex = readMsgpackFileSync<PackageFilesIndex>(indexPath)
    const pkgJsonEntry = pkgIndex.files.get('package.json')
    if (pkgJsonEntry) {
      const filePath = getFilePathByModeInCafs(storeDir, pkgJsonEntry.digest, pkgJsonEntry.mode)
      return loadJsonFileSync<DependencyManifest>(filePath)
    }
  } catch {
    // Fall through to undefined
  }
  return undefined
}
