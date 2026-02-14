import { readMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { getIndexFilePathInCafs, readManifestFromStore, type PackageFilesIndex } from '@pnpm/store.cafs'
import { type DependencyManifest } from '@pnpm/types'

/**
 * Attempts to read a package manifest from the content-addressable store (CAFS)
 * using its integrity hash. Returns `undefined` if the manifest cannot be read.
 */
export function readManifestFromCafs (
  storeDir: string,
  integrity: string,
  name: string,
  version: string
): DependencyManifest | undefined {
  try {
    const pkgId = `${name}@${version}`
    const indexPath = getIndexFilePathInCafs(storeDir, integrity, pkgId)
    const pkgIndex = readMsgpackFileSync<PackageFilesIndex>(indexPath)
    const manifest = readManifestFromStore(storeDir, pkgIndex)
    if (manifest) return manifest as DependencyManifest
  } catch {
    // Fall through to undefined
  }
  return undefined
}
