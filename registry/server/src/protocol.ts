import { createReadStream } from 'node:fs'
import type { ServerResponse } from 'node:http'
import path from 'node:path'

import type { LockfileObject } from '@pnpm/lockfile.types'

export interface PackageFileEntry {
  relativePath: string
  digest: string
  size: number
  mode: number
}

export interface PackageFilesInfo {
  integrity: string
  algo: string
  files: Record<string, { digest: string, size: number, mode: number }>
}

export interface MissingFile {
  digest: string
  size: number
  executable: boolean
  /** Absolute path to the file in the server's CAFS */
  cafsPath: string
}

export interface ResponseMetadata {
  lockfile: LockfileObject
  packageFiles: Record<string, PackageFilesInfo>
  missingDigests: string[]
  stats: {
    totalPackages: number
    alreadyInStore: number
    packagesToFetch: number
    filesInNewPackages: number
    filesAlreadyInCafs: number
    filesToDownload: number
    downloadBytes: number
  }
}

/**
 * Encode and stream the response in the pnpm-registry binary protocol.
 *
 * Format:
 *   [4 bytes: JSON metadata length (big-endian uint32)]
 *   [N bytes: JSON metadata]
 *   [file entries...]
 *   [64 zero bytes: end marker]
 *
 * Each file entry:
 *   [64 bytes: SHA-512 digest, raw binary]
 *   [4 bytes: file size (big-endian uint32)]
 *   [1 byte: 0x00=regular, 0x01=executable]
 *   [N bytes: file content]
 */
export async function encodeResponse (
  res: ServerResponse,
  metadata: ResponseMetadata,
  missingFiles: MissingFile[]
): Promise<void> {
  res.writeHead(200, {
    'Content-Type': 'application/x-pnpm-install',
  })

  // 1. Write JSON metadata
  const jsonBuffer = Buffer.from(JSON.stringify(metadata), 'utf-8')
  const lengthBuf = Buffer.alloc(4)
  lengthBuf.writeUInt32BE(jsonBuffer.length, 0)
  res.write(lengthBuf)
  res.write(jsonBuffer)

  // 2. Write file entries
  for (const file of missingFiles) {
    // Digest: 64 bytes raw binary (SHA-512 hex → binary)
    const digestBuf = Buffer.from(file.digest, 'hex')

    // Size: 4 bytes big-endian uint32
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(file.size, 0)

    // Mode: 1 byte
    const modeBuf = Buffer.alloc(1)
    modeBuf[0] = file.executable ? 0x01 : 0x00

    // Write header
    res.write(digestBuf)
    res.write(sizeBuf)
    res.write(modeBuf)

    // Write file content by streaming from CAFS
    await new Promise<void>((resolve, reject) => {
      const stream = createReadStream(file.cafsPath)
      stream.on('error', reject)
      stream.on('end', resolve)
      stream.pipe(res, { end: false })
    })
  }

  // 3. Write end marker (64 zero bytes)
  res.write(Buffer.alloc(64, 0))
  res.end()
}
