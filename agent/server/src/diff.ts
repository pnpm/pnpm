import type { LockfileObject } from '@pnpm/lockfile.types'
import { getFilePathByModeInCafs, type PackageFileInfo, type PackageFilesIndex } from '@pnpm/store.cafs'
import type { StoreIndex } from '@pnpm/store.index'

import type { MissingFile, ResponseMetadata } from './protocol.js'

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
  packageIndexBuffers: Map<string, { integrity: string, rawBuffer: Uint8Array }>
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
      const executable = (fileInfo.mode & 0o111) !== 0
      clientDigests.add(`${fileInfo.digest}:${executable ? 'x' : ''}`)
    }
  }

  // 2. Iterate resolved packages and compute missing files
  const packageIndexBuffers = new Map<string, { integrity: string, rawBuffer: Uint8Array }>()
  const missingFiles: MissingFile[] = []

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

    for (const [, fileInfo] of getFilesEntries(entry.decoded)) {
      filesInNewPackages++

      // Dedup by digest + executable flag — the same content may need to be
      // stored at both the exec and non-exec CAFS paths if different packages
      // reference it with different file modes.
      const executable = (fileInfo.mode & 0o111) !== 0
      const dedupeKey = `${fileInfo.digest}:${executable ? 'x' : ''}`
      if (!clientDigests.has(dedupeKey)) {
        clientDigests.add(dedupeKey)
        filesToDownload++
        downloadBytes += fileInfo.size
        missingFiles.push({
          digest: fileInfo.digest,
          size: fileInfo.size,
          executable,
          cafsPath: getFilePathByModeInCafs(storeDir, fileInfo.digest, fileInfo.mode),
        })
      } else {
        filesAlreadyInCafs++
      }
    }

    packageIndexBuffers.set(depPath, { integrity, rawBuffer: entry.rawBuffer })
  }

  return {
    metadata: {
      lockfile,
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

export function getFilesEntries (pkgIndex: PackageFilesIndex): Array<[string, PackageFileInfo]> {
  const { files } = pkgIndex
  if (files instanceof Map) return [...files.entries()]
  return Object.entries(files as Record<string, PackageFileInfo>)
}

function getSnapshotIntegrity (pkgSnapshot: { resolution?: { integrity?: string } | string }): string | undefined {
  if (!pkgSnapshot.resolution) return undefined
  if (typeof pkgSnapshot.resolution === 'string') return undefined
  return pkgSnapshot.resolution.integrity
}
