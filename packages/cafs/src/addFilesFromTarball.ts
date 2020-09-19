import { Duplex, PassThrough } from 'stream'
import { DeferredManifestPromise, FilesIndex, FileWriteResult } from '@pnpm/fetcher-base'
import { parseJsonStream } from './parseJson'
import decompress = require('decompress-maybe')
import tar = require('tar-stream')

export default async function (
  addStreamToCafs: (fileStream: PassThrough, mode: number) => Promise<FileWriteResult>,
  _ignore: null | ((filename: string) => Boolean),
  stream: NodeJS.ReadableStream,
  manifest?: DeferredManifestPromise
): Promise<FilesIndex> {
  const ignore = _ignore ?? (() => false)
  const extract = tar.extract()
  const filesIndex = {}
  await new Promise((resolve, reject) => {
    extract.on('entry', async (header, fileStream, next) => {
      const filename = header.name.substr(header.name.indexOf('/') + 1)
      if (header.type !== 'file' || ignore(filename)) {
        fileStream.resume()
        next()
        return
      }
      if (filename === 'package.json' && manifest) {
        parseJsonStream(fileStream, manifest)
      }
      const writeResult = addStreamToCafs(fileStream, header.mode!)
      filesIndex[filename] = {
        mode: header.mode!,
        size: header.size,
        writeResult,
      }
      next()
    })
    // listener
    extract.on('finish', () => resolve())
    extract.on('error', reject)

    // pipe through extractor
    stream
      .on('error', reject)
      .pipe(decompress() as Duplex)
      .on('error', reject).pipe(extract)
  })
  return filesIndex
}
