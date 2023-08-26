import {
  type AddToStoreResult,
  type FilesIndex,
  type FileWriteResult,
} from '@pnpm/cafs-types'
import isGzip from 'is-gzip'
import { gunzipSync } from 'zlib'
import { parseJsonBufferSync } from './parseJson'
import { parseTarball } from './parseTarball'

export function addFilesFromTarball (
  addBufferToCafs: (buffer: Buffer, mode: number) => FileWriteResult,
  _ignore: null | ((filename: string) => boolean),
  tarballBuffer: Buffer
): AddToStoreResult {
  const ignore = _ignore ?? (() => false)
  const tarContent = isGzip(tarballBuffer) ? gunzipSync(tarballBuffer) : (Buffer.isBuffer(tarballBuffer) ? tarballBuffer : Buffer.from(tarballBuffer))
  const { files } = parseTarball(tarContent)
  const filesIndex: FilesIndex = {}
  let manifestBuffer: Buffer | undefined

  for (const [relativePath, { mode, offset, size }] of files) {
    if (ignore(relativePath)) continue

    const fileBuffer = tarContent.slice(offset, offset + size)
    if (relativePath === 'package.json') {
      manifestBuffer = fileBuffer
    }
    filesIndex[relativePath] = {
      mode,
      size,
      ...addBufferToCafs(fileBuffer, mode),
    }
  }
  return {
    filesIndex,
    manifest: manifestBuffer ? parseJsonBufferSync(manifestBuffer) : undefined,
  }
}
