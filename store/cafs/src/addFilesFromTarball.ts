import { gunzipSync } from 'node:zlib'

import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import isGzip from 'is-gzip'

import { parseJsonBufferSync } from './parseJson.js'
import { parseTarball } from './parseTarball.js'

export function addFilesFromTarball (
  addBufferToCafs: (buffer: Buffer, mode: number) => FileWriteResult,
  _ignore: null | ((filename: string) => boolean),
  tarballBuffer: Buffer,
  readManifest?: boolean
): AddToStoreResult {
  const ignore = _ignore ?? (() => false)
  // chunkSize 128KB is 8x the default 16KB, reducing the number of buffer
  // allocations and copies during decompression of large tarballs.
  const tarContent = isGzip(tarballBuffer)
    ? gunzipSync(tarballBuffer, { chunkSize: 128 * 1024 })
    : tarballBuffer
  const { files } = parseTarball(tarContent)
  const filesIndex = new Map() as FilesIndex
  let manifestBuffer: Buffer | undefined

  for (const [relativePath, { mode, offset, size }] of files) {
    if (ignore(relativePath)) continue

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
