import util from 'node:util'
import { createGunzip, gunzipSync } from 'node:zlib'

import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import bz2 from 'bz2'
import isGzip from 'is-gzip'

import { parseJsonBufferSync } from './parseJson.js'
import { createTarballParser, type OnTarballFile } from './parseTarball.js'

// chunkSize 128KB (8x the Node.js default of 16KB) reduces the number of
// internal buffer allocations and copies during decompression. Benchmarks
// showed ~2.3x faster decompress at 128KB.
const GUNZIP_CHUNK_SIZE = 128 * 1024

/**
 * The largest decompressed archive held in memory whole. A larger gzip archive
 * is decompressed as a stream, so peak memory is bounded by its largest file.
 */
export const MAX_IN_MEMORY_TARBALL_SIZE = 64 * 1024 * 1024

type AddBufferToCafs = (buffer: Buffer, mode: number) => FileWriteResult

export function addFilesFromTarball (
  addBufferToCafs: AddBufferToCafs,
  tarballBuffer: Buffer,
  readManifest?: boolean,
  ignore?: (filename: string) => boolean
): AddToStoreResult {
  return addFilesFromTarContent(addBufferToCafs, decompressTarball(tarballBuffer), readManifest, ignore)
}

/**
 * Same as {@link addFilesFromTarball}, but a gzip archive that decompresses to
 * more than {@link MAX_IN_MEMORY_TARBALL_SIZE} is decompressed as a stream.
 */
export async function addFilesFromTarballBounded (
  addBufferToCafs: AddBufferToCafs,
  tarballBuffer: Buffer,
  readManifest?: boolean,
  ignore?: (filename: string) => boolean
): Promise<AddToStoreResult> {
  const tarContent = isGzip(tarballBuffer)
    ? gunzipUpTo(tarballBuffer, MAX_IN_MEMORY_TARBALL_SIZE)
    : decompressTarball(tarballBuffer)
  if (tarContent != null) {
    return addFilesFromTarContent(addBufferToCafs, tarContent, readManifest, ignore)
  }
  // Files are written to the store as they are decompressed, before the rest
  // of the archive is validated. Deferring them would mean holding them all.
  const filesIndexBuilder = createFilesIndexBuilder(addBufferToCafs, readManifest, ignore)
  const parser = createTarballParser(filesIndexBuilder.addFile)
  const gunzip = createGunzip({ chunkSize: GUNZIP_CHUNK_SIZE })
  gunzip.end(tarballBuffer)
  for await (const chunk of gunzip) {
    parser.push(chunk as Buffer)
  }
  parser.end()
  return filesIndexBuilder.result()
}

/**
 * Parses the whole archive before writing any file to the store, so a
 * malformed archive leaves nothing behind and a path that appears more than
 * once is written only once.
 */
function addFilesFromTarContent (
  addBufferToCafs: AddBufferToCafs,
  tarContent: Buffer,
  readManifest?: boolean,
  ignore?: (filename: string) => boolean
): AddToStoreResult {
  const files = new Map<string, { mode: number, content: Buffer }>()
  const parser = createTarballParser((relativePath, mode, content) => {
    files.set(relativePath, { mode, content })
  })
  parser.push(tarContent)
  parser.end()
  const filesIndexBuilder = createFilesIndexBuilder(addBufferToCafs, readManifest, ignore)
  for (const [relativePath, { mode, content }] of files) {
    filesIndexBuilder.addFile(relativePath, mode, content)
  }
  return filesIndexBuilder.result()
}

interface FilesIndexBuilder {
  addFile: OnTarballFile
  result: () => AddToStoreResult
}

function createFilesIndexBuilder (
  addBufferToCafs: AddBufferToCafs,
  readManifest?: boolean,
  ignore?: (filename: string) => boolean
): FilesIndexBuilder {
  const filesIndex = new Map() as FilesIndex
  let manifestBuffer: Buffer | undefined
  return {
    addFile: (relativePath, mode, content) => {
      if (ignore?.(relativePath)) return
      if (readManifest && relativePath === 'package.json') {
        manifestBuffer = content
      }
      filesIndex.set(relativePath, {
        mode,
        size: content.length,
        ...addBufferToCafs(content, mode),
      })
    },
    result: () => ({
      filesIndex,
      manifest: manifestBuffer ? parseJsonBufferSync(manifestBuffer) as DependencyManifest : undefined,
    }),
  }
}

/**
 * Decompresses a gzip archive whole, or returns undefined if it decompresses
 * to more than `maxSize` bytes. The size recorded in the gzip trailer rejects
 * most large archives before any decompression, and `maxOutputLength` catches
 * an archive whose trailer understates it.
 */
function gunzipUpTo (tarballBuffer: Buffer, maxSize: number): Buffer | undefined {
  if (readGzipTrailerSize(tarballBuffer) > maxSize) return undefined
  try {
    return gunzipSync(tarballBuffer, { chunkSize: GUNZIP_CHUNK_SIZE, maxOutputLength: maxSize })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_BUFFER_TOO_LARGE') {
      return undefined
    }
    throw err
  }
}

/**
 * The last four bytes of a gzip member record its decompressed size modulo 2^32.
 */
function readGzipTrailerSize (buffer: Buffer | Uint8Array): number {
  if (buffer.length < 4) return 0
  const end = buffer.length
  return (buffer[end - 4] | (buffer[end - 3] << 8) | (buffer[end - 2] << 16) | (buffer[end - 1] << 24)) >>> 0
}

function decompressTarball (tarballBuffer: Buffer): Buffer {
  if (isGzip(tarballBuffer)) {
    return gunzipSync(tarballBuffer, { chunkSize: GUNZIP_CHUNK_SIZE })
  }
  if (isBzip2(tarballBuffer)) {
    const decompressed = bz2.decompress(tarballBuffer)
    return Buffer.from(decompressed.buffer, decompressed.byteOffset, decompressed.byteLength)
  }
  // When called from a worker thread, the buffer arrives as a Uint8Array
  // (structured clone converts Buffer → Uint8Array). The tarball parser relies on
  // Buffer.prototype.toString('utf8', ...) so we must ensure it's a Buffer.
  return Buffer.isBuffer(tarballBuffer) ? tarballBuffer : Buffer.from(tarballBuffer)
}

function isBzip2 (buffer: Buffer | Uint8Array): boolean {
  return buffer.length >= 3 && buffer[0] === 0x42 && buffer[1] === 0x5a && buffer[2] === 0x68
}
