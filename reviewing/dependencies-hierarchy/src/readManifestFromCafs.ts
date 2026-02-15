import fs from 'fs'
import { getIndexFilePathInCafs, readManifestFromStore, type PackageFilesIndex } from '@pnpm/store.cafs'
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
    const pkgIndex = JSON.parse(fs.readFileSync(indexPath, 'utf8')) as PackageFilesIndex
    const manifest = readManifestFromStore(storeDir, pkgIndex)
    if (manifest) return manifest as DependencyManifest
  } catch {
    // Fall through to undefined
  }
  return undefined
}
