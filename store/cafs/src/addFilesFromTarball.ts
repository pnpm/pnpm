import type { DeferredManifestPromise, FilesIndex, FileWriteResult } from '@pnpm/cafs-types'
import isGzip from 'is-gzip'
import { gunzipSync } from 'zlib'
import { parseJsonBufferSync } from './parseJson'
import { parseTarball } from './parseTarball'

export function addFilesFromTarball (
  addBufferToCafs: (buffer: Buffer, mode: number) => FileWriteResult,
  _ignore: null | ((filename: string) => boolean),
  tarballBuffer: Buffer,
  manifest?: DeferredManifestPromise
): FilesIndex {
  const ignore = _ignore ?? (() => false)
  const tarContent = isGzip(tarballBuffer) ? gunzipSync(tarballBuffer) : tarballBuffer
  const { files } = parseTarball(tarContent)
  const filesIndex: FilesIndex = {}
  let manifestBuffer: Buffer | undefined

  for (const [relativePath, { mode, offset, size }] of files) {
    if (ignore(relativePath)) continue

    const fileBuffer = tarContent.slice(offset, offset + size)
    const writeResult = addBufferToCafs(fileBuffer, mode)
    if (relativePath === 'package.json' && (manifest != null)) {
      manifestBuffer = fileBuffer
    }
    filesIndex[relativePath] = {
      mode,
      size,
      writeResult: Promise.resolve(writeResult),
    }
  }
  if (!filesIndex['package.json'] && manifest != null) {
    manifest.resolve(undefined)
  } else if (manifestBuffer && manifest) {
    manifest.resolve(parseJsonBufferSync(manifestBuffer))
  }
  return filesIndex
}
