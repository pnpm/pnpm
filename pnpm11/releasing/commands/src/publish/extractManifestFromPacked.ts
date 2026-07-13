import fs from 'node:fs'
import path from 'node:path'
import { createGunzip } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import tar from 'tar-stream'

const TARBALL_SUFFIXES = ['.tar.gz', '.tgz'] as const

export type TarballSuffix = typeof TARBALL_SUFFIXES[number]
export type TarballPath = `${string}${TarballSuffix}`

export const isTarballPath = (path: string): path is TarballPath =>
  TARBALL_SUFFIXES.some(suffix => path.endsWith(suffix))

export async function extractManifestFromPacked<Output = ExportedManifest> (tarballPath: TarballPath): Promise<Output> {
  const { manifest } = await extractEntriesFromPacked(tarballPath)
  return JSON.parse(manifest)
}

/**
 * Read the publish manifest from a pre-built tarball, filling in its `readme` from the tarball's
 * root README file when the manifest doesn't already declare one. This mirrors the npm CLI, which
 * reads the readme out of the tarball (via pacote's `fullReadJson`) so the registry gets it as
 * metadata even though it isn't stored in the packed `package.json`.
 */
export async function extractPublishManifestFromPacked (tarballPath: TarballPath): Promise<ExportedManifest> {
  const { manifest, readme } = await extractEntriesFromPacked(tarballPath)
  const parsed = JSON.parse(manifest) as ExportedManifest
  if (parsed.readme == null && readme != null) {
    parsed.readme = readme
  }
  return parsed
}

interface PackedEntries {
  manifest: string
  readme?: string
}

async function extractEntriesFromPacked (tarballPath: TarballPath): Promise<PackedEntries> {
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

  const promise = new Promise<PackedEntries>((resolve, reject) => {
    function handleError (error: unknown): void {
      cleanup()
      reject(error)
    }

    tarballStream.once('error', handleError)
    gunzip.once('error', handleError)

    let manifest: string | undefined
    let readme: string | undefined

    extract.on('entry', (header, stream, next) => {
      const normalizedPath = path.normalize(header.name).replaceAll('\\', '/')
      const isManifest = normalizedPath === 'package/package.json'
      const isReadme = /^package\/readme\.md$/i.test(normalizedPath)

      if (!isManifest && !isReadme) {
        stream.once('end', next)
        stream.resume()
        return
      }

      const chunks: Buffer[] = []
      stream.on('data', (chunk: Buffer) => {
        chunks.push(chunk)
      })

      stream.once('end', () => {
        const text = Buffer.concat(chunks).toString()
        if (isManifest) {
          manifest = text
        } else {
          readme = text
        }
        next()
      })

      stream.once('error', handleError)
    })

    extract.once('finish', () => {
      cleanup()

      if (manifest == null) {
        reject(new PublishArchiveMissingManifestError(tarballPath))
        return
      }
      resolve({ manifest, readme })
    })

    extract.once('error', handleError)
  })

  tarballStream.pipe(gunzip).pipe(extract)

  return promise
}

export class PublishArchiveMissingManifestError extends PnpmError {
  readonly tarballPath: string
  constructor (tarballPath: string) {
    super('PUBLISH_ARCHIVE_MISSING_MANIFEST', `The archive ${tarballPath} does not contain package/package.json`)
    this.tarballPath = tarballPath
  }
}
