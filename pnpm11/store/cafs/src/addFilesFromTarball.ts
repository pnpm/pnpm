import { gunzipSync } from 'node:zlib'

import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import bz2 from 'bz2'
import isGzip from 'is-gzip'

import { parseJsonBufferSync } from './parseJson.js'
import { parseTarball } from './parseTarball.js'

export function addFilesFromTarball (
  addBufferToCafs: (buffer: Buffer, mode: number) => FileWriteResult,
  tarballBuffer: Buffer,
  readManifest?: boolean,
  ignore?: (filename: string) => boolean
): AddToStoreResult {
  const tarContent = decompressTarball(tarballBuffer)
  const { files } = parseTarball(tarContent)
  const filesIndex = new Map() as FilesIndex
  let manifestBuffer: Buffer | undefined

  for (const [relativePath, { mode, offset, size }] of files) {
    if (ignore?.(relativePath)) continue

    const fileBuffer = tarContent.subarray(offset, offset + size)
    if (readManifest && relativePath === 'package.json') {
      manifestBuffer = fileBuffer
    }
    filesIndex.set(relativePath, {
      mode,
      size,
      ...addBufferToCafs(fileBuffer, mode),
    })
  }
  return {
    filesIndex,
    manifest: manifestBuffer ? parseJsonBufferSync(manifestBuffer) as DependencyManifest : undefined,
  }
}

function decompressTarball (tarballBuffer: Buffer): Buffer {
  if (isGzip(tarballBuffer)) {
    // chunkSize 128KB (8x the Node.js default of 16KB) reduces the number of
    // internal buffer allocations and copies during decompression. Benchmarks
    // showed ~2.3x faster decompress at 128KB.
    return gunzipSync(tarballBuffer, { chunkSize: 128 * 1024 })
  }
  if (isBzip2(tarballBuffer)) {
    const decompressed = bz2.decompress(tarballBuffer)
    return Buffer.from(decompressed.buffer, decompressed.byteOffset, decompressed.byteLength)
  }
  // When called from a worker thread, the buffer arrives as a Uint8Array
  // (structured clone converts Buffer → Uint8Array). parseTarball relies on
  // Buffer.prototype.toString('utf8', ...) so we must ensure it's a Buffer.
  return Buffer.isBuffer(tarballBuffer) ? tarballBuffer : Buffer.from(tarballBuffer)
}

function isBzip2 (buffer: Buffer | Uint8Array): boolean {
  return buffer.length >= 3 && buffer[0] === 0x42 && buffer[1] === 0x5a && buffer[2] === 0x68
}
