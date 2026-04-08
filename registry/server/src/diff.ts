import type { LockfileObject } from '@pnpm/lockfile.types'
import { getFilePathByModeInCafs, type PackageFileInfo, type PackageFilesIndex } from '@pnpm/store.cafs'
import type { StoreIndex } from '@pnpm/store.index'

import type { MissingFile, PackageFilesInfo, ResponseMetadata } from './protocol.js'

export interface IntegrityEntry {
  decoded: PackageFilesIndex
  rawBuffer: Uint8Array
}

/**
 * Build an index mapping integrity hashes to their PackageFilesIndex
 * (decoded for diff computation) and raw msgpack buffer (for sending to client).
 */
export function buildIntegrityIndex (storeIndex: StoreIndex): Map<string, IntegrityEntry> {
  const index = new Map<string, IntegrityEntry>()
  for (const [key, value] of storeIndex.entries()) {
    const tabIdx = key.indexOf('\t')
    if (tabIdx === -1) continue
    const integrity = key.slice(0, tabIdx)
    if (index.has(integrity)) continue
    const rawBuffer = storeIndex.getRaw(key)
    if (!rawBuffer) continue
    index.set(integrity, {
      decoded: value as PackageFilesIndex,
      rawBuffer,
    })
  }
  return index
}

export interface DiffResult {
  metadata: ResponseMetadata
  missingFiles: MissingFile[]
  /** Pre-packed msgpack buffers for each package, keyed by depPath */
  packageIndexBuffers: Map<string, Uint8Array>
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
  integrityIndex: Map<string, IntegrityEntry>,
  storeDir: string
): DiffResult {
  // 1. Build the set of file digests the client already has
  const clientDigests = new Set<string>()
  const clientIntegrities = new Set(storeIntegrities)

  for (const integrity of storeIntegrities) {
    const entry = integrityIndex.get(integrity)
    if (!entry) continue
    for (const [, fileInfo] of getFilesEntries(entry.decoded)) {
      clientDigests.add(fileInfo.digest)
    }
  }

  // 2. Iterate resolved packages and compute missing files
  const packageFiles: Record<string, PackageFilesInfo> = {}
  const packageIndexBuffers = new Map<string, Uint8Array>()
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

    const entry = integrityIndex.get(integrity)
    if (!entry) continue // package not indexed on server yet

    packagesToFetch++
    const filesRecord: Record<string, { digest: string, size: number, mode: number }> = {}

    for (const [relativePath, fileInfo] of getFilesEntries(entry.decoded)) {
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
      algo: entry.decoded.algo,
      files: filesRecord,
    }
    packageIndexBuffers.set(depPath, entry.rawBuffer)
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
    packageIndexBuffers,
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
