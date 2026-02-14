import fs from 'fs'
import { createGunzip } from 'zlib'
import path from 'path'
import tar from 'tar-stream'
import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'

const TARBALL_SUFFIXES = ['.tar.gz', '.tgz'] as const

export type TarballSuffix = typeof TARBALL_SUFFIXES[number]
export type TarballPath = `${string}${TarballSuffix}`

export const isTarballPath = (path: string): path is TarballPath =>
  TARBALL_SUFFIXES.some(suffix => path.endsWith(suffix))

export async function extractManifestFromPacked<Output = ExportedManifest> (tarballPath: TarballPath): Promise<Output> {
  const extract = tar.extract()
  const gunzip = createGunzip()
  const tarballStream = fs.createReadStream(tarballPath)

  let cleanedUp = false

  function cleanup (): void {
    if (cleanedUp) return
    cleanedUp = true

    extract.destroy()
    gunzip.destroy()
    tarballStream.destroy()
  }

  const promise = new Promise<string>((resolve, reject) => {
    function handleError (error: unknown): void {
      cleanup()
      reject(error)
    }

    tarballStream.once('error', handleError)
    gunzip.once('error', handleError)

    let manifestFound = false

    extract.on('entry', (header, stream, next) => {
      const normalizedPath = path.normalize(header.name).replaceAll('\\', '/')

      if (normalizedPath !== 'package/package.json') {
        stream.once('end', next)
        stream.resume()
        return
      }

      manifestFound = true

      const chunks: Buffer[] = []
      stream.on('data', (chunk: Buffer) => {
        chunks.push(chunk)
      })

      stream.once('end', () => {
        try {
          const text = Buffer.concat(chunks).toString()
          cleanup()
          resolve(text)
        } catch (error) {
          handleError(error)
        }
      })

      stream.once('error', handleError)
    })

    extract.once('finish', () => {
      cleanup()

      if (!manifestFound) {
        reject(new PublishArchiveMissingManifestError(tarballPath))
      }
    })

    extract.once('error', handleError)
  })

  tarballStream.pipe(gunzip).pipe(extract)

  return JSON.parse(await promise)
}

export class PublishArchiveMissingManifestError extends PnpmError {
  readonly tarballPath: string
  constructor (tarballPath: string) {
    super('PUBLISH_ARCHIVE_MISSING_MANIFEST', `The archive ${tarballPath} does not contain package/package.json`)
    this.tarballPath = tarballPath
  }
}
