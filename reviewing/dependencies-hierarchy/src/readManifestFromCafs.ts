import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { type StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DependencyManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'

/**
 * Attempts to read a package manifest from the content-addressable store (CAFS)
 * using its integrity hash. Returns `undefined` if the manifest cannot be read.
 */
export function readManifestFromCafs (storeDir: string, storeIndex: StoreIndex, pkg: {
  integrity: string
  name: string
  version: string
}): DependencyManifest | undefined {
  try {
    const pkgId = `${pkg.name}@${pkg.version}`
    const indexPath = storeIndexKey(pkg.integrity, pkgId)
    const pkgIndex = storeIndex.get(indexPath) as PackageFilesIndex | undefined
    if (!pkgIndex) return undefined
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
