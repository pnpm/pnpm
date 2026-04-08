import type { LockfileObject } from '@pnpm/lockfile.types'
import { getFilePathByModeInCafs, type PackageFileInfo, type PackageFilesIndex } from '@pnpm/store.cafs'
import type { StoreIndex } from '@pnpm/store.index'

import type { MissingFile, PackageFilesInfo, ResponseMetadata } from './protocol.js'

/**
 * Build an index mapping integrity hashes to their PackageFilesIndex.
 * This is called once at startup and updated as new packages are fetched.
 */
export function buildIntegrityIndex (storeIndex: StoreIndex): Map<string, PackageFilesIndex> {
  const index = new Map<string, PackageFilesIndex>()
  for (const [key, value] of storeIndex.entries()) {
    const tabIdx = key.indexOf('\t')
    if (tabIdx === -1) continue
    const integrity = key.slice(0, tabIdx)
    index.set(integrity, value as PackageFilesIndex)
  }
  return index
}

export interface DiffResult {
  metadata: ResponseMetadata
  missingFiles: MissingFile[]
}

/**
 * Given a resolved lockfile, the client's store integrities, and the server's
 * integrity index, compute which files need to be sent to the client.
 *
 * The algorithm:
 * 1. Union all file digests from the client's existing packages
 * 2. For each package in the lockfile, look up its file index
 * 3. Any file digest NOT in the client's set is "missing" and must be sent
 * 4. Dedup within the response (same digest only sent once)
 */
export function computeDiff (
  lockfile: LockfileObject,
  storeIntegrities: string[],
  integrityIndex: Map<string, PackageFilesIndex>,
  storeDir: string
): DiffResult {
  // 1. Build the set of file digests the client already has
  const clientDigests = new Set<string>()
  const clientIntegrities = new Set(storeIntegrities)

  for (const integrity of storeIntegrities) {
    const pkgIndex = integrityIndex.get(integrity)
    if (!pkgIndex) continue
    for (const [, fileInfo] of getFilesEntries(pkgIndex)) {
      clientDigests.add(fileInfo.digest)
    }
  }

  // 2. Iterate resolved packages and compute missing files
  const packageFiles: Record<string, PackageFilesInfo> = {}
  const missingFiles: MissingFile[] = []
  const missingDigests: string[] = []

  let totalPackages = 0
  let alreadyInStore = 0
  let packagesToFetch = 0
  let filesInNewPackages = 0
  let filesAlreadyInCafs = 0
  let filesToDownload = 0
  let downloadBytes = 0

  for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {})) {
    totalPackages++
    const integrity = getSnapshotIntegrity(pkgSnapshot)
    if (!integrity) continue

    // Client already has this exact package — skip entirely
    if (clientIntegrities.has(integrity)) {
      alreadyInStore++
      continue
    }

    const pkgIndex = integrityIndex.get(integrity)
    if (!pkgIndex) continue // package not indexed on server yet

    packagesToFetch++
    const filesRecord: Record<string, { digest: string, size: number, mode: number }> = {}

    for (const [relativePath, fileInfo] of getFilesEntries(pkgIndex)) {
      filesInNewPackages++
      filesRecord[relativePath] = {
        digest: fileInfo.digest,
        size: fileInfo.size,
        mode: fileInfo.mode,
      }

      if (!clientDigests.has(fileInfo.digest)) {
        clientDigests.add(fileInfo.digest) // dedup within response
        filesToDownload++
        downloadBytes += fileInfo.size
        missingDigests.push(fileInfo.digest)
        missingFiles.push({
          digest: fileInfo.digest,
          size: fileInfo.size,
          executable: (fileInfo.mode & 0o111) !== 0,
          cafsPath: getFilePathByModeInCafs(storeDir, fileInfo.digest, fileInfo.mode),
        })
      } else {
        filesAlreadyInCafs++
      }
    }

    packageFiles[depPath] = {
      integrity,
      algo: pkgIndex.algo,
      files: filesRecord,
    }
  }

  return {
    metadata: {
      lockfile,
      packageFiles,
      missingDigests,
      stats: {
        totalPackages,
        alreadyInStore,
        packagesToFetch,
        filesInNewPackages,
        filesAlreadyInCafs,
        filesToDownload,
        downloadBytes,
      },
    },
    missingFiles,
  }
}

function getFilesEntries (pkgIndex: PackageFilesIndex): Array<[string, PackageFileInfo]> {
  const { files } = pkgIndex
  if (files instanceof Map) return [...files.entries()]
  return Object.entries(files as Record<string, PackageFileInfo>)
}

function getSnapshotIntegrity (pkgSnapshot: { resolution?: { integrity?: string } | string }): string | undefined {
  if (!pkgSnapshot.resolution) return undefined
  if (typeof pkgSnapshot.resolution === 'string') return undefined
  return pkgSnapshot.resolution.integrity
}
