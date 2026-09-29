import fs from 'node:fs'
import path from 'node:path'
import { createGunzip } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import { type ExportedManifest, getReadmeRank, isPreferredReadme, type ReadmeCandidate } from '@pnpm/releasing.exportable-manifest'
import tar from 'tar-stream'

const TARBALL_SUFFIXES = ['.tar.gz', '.tgz'] as const
const README_MD_RANK = getReadmeRank('README.md')

export type TarballSuffix = typeof TARBALL_SUFFIXES[number]
export type TarballPath = `${string}${TarballSuffix}`

export const isTarballPath = (path: string): path is TarballPath =>
  TARBALL_SUFFIXES.some(suffix => path.endsWith(suffix))

export async function extractManifestFromPacked<Output = ExportedManifest> (tarballPath: TarballPath): Promise<Output> {
  const { manifest } = await extractEntriesFromPacked(tarballPath, false)
  return JSON.parse(manifest)
}

/**
 * Read the publish manifest from a pre-built tarball, filling in its `readme` from the tarball's
 * root README file when the manifest doesn't already declare one. This mirrors the npm CLI, which
 * reads the readme out of the tarball (via pacote's `fullReadJson`) so the registry gets it as
 * metadata even though it isn't stored in the packed `package.json`.
 */
export async function extractPublishManifestFromPacked (tarballPath: TarballPath): Promise<ExportedManifest> {
  const { manifest, readme } = await extractEntriesFromPacked(tarballPath, true)
  const parsed = JSON.parse(manifest) as ExportedManifest
  if (parsed.readme == null && readme != null) {
    parsed.readme = readme.text
  }
  return parsed
}

interface PackedEntries {
  manifest: string
  readme?: PackedReadme
}

interface PackedReadme extends ReadmeCandidate {
  text: string
}

/**
 * Read `package/package.json` from the tarball and, when `wantReadme` is set, the package-root
 * README npm would pick. Rejects with `PublishArchiveMissingManifestError` when the archive has no
 * manifest, and with the stream error when the archive cannot be read.
 */
async function extractEntriesFromPacked (tarballPath: TarballPath, wantReadme: boolean): Promise<PackedEntries> {
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
    let settled = false
    let manifest: string | undefined
    let readme: PackedReadme | undefined

    function handleError (error: unknown): void {
      cleanup()
      reject(error)
    }

    function settle (): void {
      if (settled) return
      settled = true
      cleanup()
      if (manifest == null) {
        reject(new PublishArchiveMissingManifestError(tarballPath))
        return
      }
      resolve({ manifest, readme })
    }

    tarballStream.once('error', handleError)
    gunzip.once('error', handleError)

    extract.on('entry', (header, stream, next) => {
      const normalizedPath = path.normalize(header.name).replaceAll('\\', '/')
      const isManifest = normalizedPath === 'package/package.json'
      const readmeCandidate = wantReadme && !isManifest && header.type === 'file'
        ? getRootReadmeCandidate(normalizedPath)
        : undefined
      const wantedReadme = readmeCandidate != null && isPreferredReadme(readmeCandidate, readme)
        ? readmeCandidate
        : undefined

      if (!isManifest && wantedReadme == null) {
        stream.once('end', next)
        stream.resume()
        return
      }

      const chunks: Buffer[] = []
      stream.on('data', (chunk) => {
        chunks.push(chunk as Buffer)
      })

      stream.once('end', () => {
        const text = Buffer.concat(chunks).toString()
        if (wantedReadme == null) {
          manifest = text
        } else {
          readme = { ...wantedReadme, text }
        }
        // Stop early once every wanted entry has been captured, so the rest of the tarball isn't
        // decompressed. Nothing outranks a README.md.
        if (manifest != null && (!wantReadme || readme?.rank === README_MD_RANK)) {
          settle()
          return
        }
        next()
      })

      stream.once('error', handleError)
    })

    extract.once('finish', settle)
    extract.once('error', handleError)
  })

  tarballStream.pipe(gunzip).pipe(extract)

  return promise
}

function getRootReadmeCandidate (normalizedPath: string): ReadmeCandidate | undefined {
  if (!normalizedPath.startsWith('package/')) return undefined
  const fileName = normalizedPath.slice('package/'.length)
  if (fileName.includes('/')) return undefined
  const rank = getReadmeRank(fileName)
  return rank == null ? undefined : { fileName, rank }
}

export class PublishArchiveMissingManifestError extends PnpmError {
  readonly tarballPath: string
  constructor (tarballPath: string) {
    super('PUBLISH_ARCHIVE_MISSING_MANIFEST', `The archive ${tarballPath} does not contain package/package.json`)
    this.tarballPath = tarballPath
  }
}
