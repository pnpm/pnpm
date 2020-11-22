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
  await new Promise<void>((resolve, reject) => {
    extract.on('entry', (header, fileStream, next) => {
      // There are some edge cases, where the same files are extracted multiple times.
      // So there will be an entry for "lib/index.js" and another one for "lib//index.js",
      // which are the same file.
      // Hence, we are normalizing the file name, replacing // with / and checking for duplicates.
      // Example of such package: @pnpm/colorize-semver-diff@1.0.1
      const filename = header.name.substr(header.name.indexOf('/') + 1).replace(/\/\//g, '/')
      if (header.type !== 'file' || ignore(filename) || filesIndex[filename]) {
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
