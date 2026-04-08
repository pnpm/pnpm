import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import type { ServerResponse } from 'node:http'
import path from 'node:path'
import { gzipSync } from 'node:zlib'

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
  // Build the binary payload in memory, then gzip and send
  const parts: Buffer[] = []

  // 1. JSON metadata (empty object if null — file-only response)
  const jsonBuffer = Buffer.from(JSON.stringify(_metadata ?? {}), 'utf-8')
  const lengthBuf = Buffer.alloc(4)
  lengthBuf.writeUInt32BE(jsonBuffer.length, 0)
  parts.push(lengthBuf)
  parts.push(jsonBuffer)

  // 2. File entries — use compressed cache for faster reads + smaller transfer
  for (const file of missingFiles) {
    const content = readCompressed(file.cafsPath)
    const digestBuf = Buffer.from(file.digest, 'hex')
    // Use compressed size in the header so client knows how many bytes to read
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(content.length, 0)
    const modeBuf = Buffer.alloc(1)
    // Bit 0: executable, Bit 1: compressed
    modeBuf[0] = (file.executable ? 0x01 : 0x00) | 0x02

    parts.push(digestBuf)
    parts.push(sizeBuf)
    parts.push(modeBuf)
    parts.push(content)
  }

  // 3. End marker (64 zero bytes)
  parts.push(Buffer.alloc(64, 0))

  const payload = Buffer.concat(parts)

  res.writeHead(200, {
    'Content-Type': 'application/x-pnpm-install',
    'Content-Length': payload.length,
  })
  res.end(payload)
}

/**
 * Read a CAFS file, returning its gzip-compressed form.
 * On first read, compresses and caches as a .gz sibling file.
 * Subsequent reads serve the cached .gz directly — smaller disk reads.
 */
function readCompressed (cafsPath: string): Buffer {
  const gzPath = `${cafsPath}.gz`
  if (existsSync(gzPath)) {
    return readFileSync(gzPath)
  }
  const raw = readFileSync(cafsPath)
  const compressed = gzipSync(raw, { level: 1 }) // level 1 = fast
  try {
    mkdirSync(path.dirname(gzPath), { recursive: true })
    writeFileSync(gzPath, compressed)
  } catch {
    // non-critical — if caching fails, just return the compressed data
  }
  return compressed
}
