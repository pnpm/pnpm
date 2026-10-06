import { createReadStream, promises as fs } from 'node:fs'
import { createGunzip, gunzipSync } from 'node:zlib'

import { isError } from '@pnpm/error'
import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import isGzip from 'is-gzip'
import Bunzip from 'seek-bzip'

import { type Bzip2Input, decodeBzip2, openBzip2Input } from './bzip2.js'
import { createTarballFileWriterFactory, type TarballFileWriterFactory } from './createTarballFileWriter.js'
import { parseJsonBufferSync } from './parseJson.js'
import { type CreateTarballFileWriter, createTarballParser, type OnTarballFile } from './parseTarball.js'

// chunkSize 128KB (8x the Node.js default of 16KB) reduces the number of
// internal buffer allocations and copies during decompression. Benchmarks
// showed ~2.3x faster decompress at 128KB.
const GUNZIP_CHUNK_SIZE = 128 * 1024

/**
 * The largest decompressed archive held in memory whole. A larger gzip archive
 * is decompressed as a stream. Large regular files are written in chunks;
 * manifests and TAR metadata are read in memory.
 */
export const MAX_IN_MEMORY_TARBALL_SIZE = 64 * 1024 * 1024

type AddBufferToCafs = (buffer: Buffer, mode: number) => FileWriteResult

type TarballImportOptions = {
  readManifest?: boolean
  ignore?: (filename: string) => boolean
  storeDir: string
}

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
 * A streamed archive's files are written to the store as they are read, so
 * when it is corrupt or truncated, the files before the failure stay in the
 * store unreferenced.
 */
export async function addFilesFromTarballBounded (
  addBufferToCafs: AddBufferToCafs,
  tarballBuffer: Buffer,
  opts: TarballImportOptions
): Promise<AddToStoreResult> {
  const { readManifest, ignore, storeDir } = opts
  if (isBzip2(tarballBuffer)) return addFilesFromBzip2(addBufferToCafs, tarballBuffer, opts)
  const tarContent = isGzip(tarballBuffer)
    ? gunzipUpTo(tarballBuffer, MAX_IN_MEMORY_TARBALL_SIZE)
    : decompressTarball(tarballBuffer)
  if (tarContent != null) {
    return addFilesFromTarContent(addBufferToCafs, tarContent, readManifest, ignore)
  }
  const writers = createTarballFileWriterFactory(storeDir)
  const filesIndexBuilder = createFilesIndexBuilder(addBufferToCafs, { readManifest, ignore, writers })
  const parser = createTarballParser(filesIndexBuilder.addFile, filesIndexBuilder.createFileWriter, MAX_IN_MEMORY_TARBALL_SIZE)
  const gunzip = createGunzip({ chunkSize: GUNZIP_CHUNK_SIZE })
  gunzip.end(tarballBuffer)
  try {
    for await (const chunk of gunzip) parser.push(chunk as Buffer)
    parser.end()
    await writers.publish()
    return filesIndexBuilder.result()
  } finally {
    gunzip.destroy()
    writers.cleanup()
  }
}

export async function addFilesFromTarballFile (
  addBufferToCafs: AddBufferToCafs,
  tarballFile: string,
  { readManifest, ignore, storeDir }: TarballImportOptions
): Promise<AddToStoreResult> {
  const handle = await fs.open(tarballFile, 'r')
  const prefix = Buffer.alloc(3)
  try {
    await handle.read(prefix, 0, prefix.length, 0)
  } finally {
    await handle.close()
  }
  if (isBzip2(prefix)) {
    const source = openBzip2Input(tarballFile)
    try {
      return await addFilesFromBzip2(addBufferToCafs, source.input, { readManifest, ignore, storeDir })
    } finally {
      source.close()
    }
  }
  const source = createReadStream(tarballFile)
  const stream = isGzip(prefix) ? source.pipe(createGunzip({ chunkSize: GUNZIP_CHUNK_SIZE })) : source
  source.on('error', (error) => {
    stream.destroy(error)
  })
  const writers = createTarballFileWriterFactory(storeDir)
  const filesIndexBuilder = createFilesIndexBuilder(addBufferToCafs, { readManifest, ignore, writers })
  const parser = createTarballParser(filesIndexBuilder.addFile, filesIndexBuilder.createFileWriter, MAX_IN_MEMORY_TARBALL_SIZE)
  try {
    for await (const chunk of stream) parser.push(chunk as Buffer)
    parser.end()
    await writers.publish()
    return filesIndexBuilder.result()
  } finally {
    source.destroy()
    stream.destroy()
    writers.cleanup()
  }
}

async function addFilesFromBzip2 (
  addBufferToCafs: AddBufferToCafs,
  input: Buffer | Bzip2Input,
  { readManifest, ignore, storeDir }: TarballImportOptions
): Promise<AddToStoreResult> {
  const writers = createTarballFileWriterFactory(storeDir)
  const filesIndexBuilder = createFilesIndexBuilder(addBufferToCafs, { readManifest, ignore, writers })
  const parser = createTarballParser(filesIndexBuilder.addFile, filesIndexBuilder.createFileWriter, MAX_IN_MEMORY_TARBALL_SIZE)
  try {
    decodeBzip2(input, parser)
    await writers.publish()
    return filesIndexBuilder.result()
  } finally {
    writers.cleanup()
  }
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
  const filesIndexBuilder = createFilesIndexBuilder(addBufferToCafs, { readManifest, ignore })
  for (const [relativePath, { mode, content }] of files) {
    filesIndexBuilder.addFile(relativePath, mode, content)
  }
  return filesIndexBuilder.result()
}

interface FilesIndexBuilder {
  addFile: OnTarballFile
  createFileWriter?: CreateTarballFileWriter
  result: () => AddToStoreResult
}

function createFilesIndexBuilder (
  addBufferToCafs: AddBufferToCafs,
  { readManifest, ignore, writers }: Pick<TarballImportOptions, 'readManifest' | 'ignore'> & { writers?: TarballFileWriterFactory }
): FilesIndexBuilder {
  const filesIndex = new Map() as FilesIndex
  let manifestBuffer: Buffer | undefined
  return {
    createFileWriter: (relativePath, mode, size) => createEntryWriter({ writers, filesIndex, readManifest, ignore }, { relativePath, mode, size }),
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

function createEntryWriter (
  opts: { writers?: TarballFileWriterFactory, filesIndex: FilesIndex, readManifest?: boolean, ignore?: (filename: string) => boolean },
  entry: { relativePath: string, mode: number, size: number }
): ReturnType<CreateTarballFileWriter> {
  if (opts.ignore?.(entry.relativePath)) return { write: () => {}, end: () => {} }
  if (!opts.writers || entry.size <= MAX_IN_MEMORY_TARBALL_SIZE) return undefined
  if (opts.readManifest && entry.relativePath === 'package.json') return undefined
  return opts.writers.create(entry.mode, (file) => {
    opts.filesIndex.set(entry.relativePath, { mode: entry.mode, size: entry.size, ...file })
  })
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
    if (isError(err) && 'code' in err && err.code === 'ERR_BUFFER_TOO_LARGE') {
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
    return Bunzip.decode(tarballBuffer)
  }
  // When called from a worker thread, the buffer arrives as a Uint8Array
  // (structured clone converts Buffer → Uint8Array). The tarball parser relies on
  // Buffer.prototype.toString('utf8', ...) so we must ensure it's a Buffer.
  return Buffer.isBuffer(tarballBuffer) ? tarballBuffer : Buffer.from(tarballBuffer)
}

function isBzip2 (buffer: Buffer | Uint8Array): boolean {
  return buffer.length >= 3 && buffer[0] === 0x42 && buffer[1] === 0x5a && buffer[2] === 0x68
}
