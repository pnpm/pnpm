import { readFileSync } from 'node:fs'
import type { ServerResponse } from 'node:http'

import type { LockfileObject } from '@pnpm/lockfile.types'

export interface MissingFile {
  digest: string
  size: number
  executable: boolean
  /** Absolute path to the file in the server's CAFS */
  cafsPath: string
}

export interface MissingDigestInfo {
  digest: string
  size: number
  executable: boolean
}

export interface ResponseMetadata {
  lockfile: LockfileObject
  missingFiles: MissingDigestInfo[]
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
export function encodeResponse (
  res: ServerResponse,
  _metadata: ResponseMetadata | null,
  missingFiles: MissingFile[]
): void {
  res.writeHead(200, {
    'Content-Type': 'application/x-pnpm-install',
    'Transfer-Encoding': 'chunked',
  })

  // 1. JSON metadata
  const jsonBuffer = Buffer.from(JSON.stringify(_metadata ?? {}), 'utf-8')
  const lengthBuf = Buffer.alloc(4)
  lengthBuf.writeUInt32BE(jsonBuffer.length, 0)
  res.write(lengthBuf)
  res.write(jsonBuffer)

  // 2. Stream file entries — each written immediately after reading
  for (const file of missingFiles) {
    const content = readFileSync(file.cafsPath)
    const headerBuf = Buffer.alloc(69) // 64 digest + 4 size + 1 mode
    Buffer.from(file.digest, 'hex').copy(headerBuf, 0)
    headerBuf.writeUInt32BE(content.length, 64)
    headerBuf[68] = file.executable ? 0x01 : 0x00
    res.write(headerBuf)
    res.write(content)
  }

  // 3. End marker
  res.write(Buffer.alloc(64, 0))
  res.end()
}

