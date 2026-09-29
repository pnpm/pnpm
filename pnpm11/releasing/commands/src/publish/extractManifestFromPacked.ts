import fs from 'node:fs'
import path from 'node:path'
import { createGunzip } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import { type ExportedManifest, isMarkdownReadmeFileName, isReadmeFileName } from '@pnpm/releasing.exportable-manifest'
import tar from 'tar-stream'

const TARBALL_SUFFIXES = ['.tar.gz', '.tgz'] as const

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
  const file = await fs.promises.open(tarballPath, 'r')
  try {
    const { manifest, readme, fallbackReadmeIndex } = await extractEntriesFromPacked(tarballPath, true, file)
    const parsed = JSON.parse(manifest) as ExportedManifest
    if (parsed.readme == null) {
      const selectedReadme = readme ?? (fallbackReadmeIndex == null
        ? undefined
        : await readPackedEntry(file, tarballPath, fallbackReadmeIndex))
      if (selectedReadme != null) parsed.readme = selectedReadme
    }
    return parsed
  } finally {
    await file.close()
  }
}

interface PackedEntries {
  manifest: string
  readme?: string
  fallbackReadmeIndex?: number
}

/**
 * Scan the tarball for `package/package.json` and, when `wantReadme` is set, a root README
 * recognized by npm. The manifest-only path (`wantReadme` false) resolves as soon as the
 * manifest entry is read and stops decompressing the rest of the archive; the publish path scans
 * on until it finds the preferred README.md or reaches the end.
 */
async function extractEntriesFromPacked (
  tarballPath: TarballPath,
  wantReadme: boolean,
  file?: fs.promises.FileHandle
): Promise<PackedEntries> {
  const extract = tar.extract()
  const gunzip = createGunzip()
  const tarballStream = file?.createReadStream({ start: 0, autoClose: false }) ?? fs.createReadStream(tarballPath)

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
    let readme: string | undefined
    let fallbackReadmeIndex: number | undefined
    let fallbackIsMarkdown = false
    let entryIndex = 0

    function handleError (error: unknown): void {
      cleanup()
      reject(error)
    }

    function settle (archiveFinished = false): void {
      if (settled) return
      settled = true
      // Destroying a FileHandle read stream closes the descriptor needed by the fallback pass.
      if (!archiveFinished) cleanup()
      if (manifest == null) {
        reject(new PublishArchiveMissingManifestError(tarballPath))
        return
      }
      resolve({ manifest, readme, fallbackReadmeIndex })
    }

    tarballStream.once('error', handleError)
    gunzip.once('error', handleError)

    extract.on('entry', (header, stream, next) => {
      const currentIndex = entryIndex++
      const normalizedPath = path.normalize(header.name).replaceAll('\\', '/')
      const isManifest = normalizedPath === 'package/package.json'
      const readmeFileName = normalizedPath.startsWith('package/')
        ? normalizedPath.slice('package/'.length)
        : ''
      const isReadme = wantReadme &&
        header.type === 'file' &&
        !readmeFileName.includes('/') &&
        isReadmeFileName(readmeFileName)
      const isPreferredReadme = isReadme && /^readme\.md$/i.test(readmeFileName)
      const isMarkdownReadme = isReadme && isMarkdownReadmeFileName(readmeFileName)

      if (isReadme && !isPreferredReadme && readme == null &&
        (fallbackReadmeIndex == null || (isMarkdownReadme && !fallbackIsMarkdown))) {
        fallbackReadmeIndex = currentIndex
        fallbackIsMarkdown = isMarkdownReadme
      }
      if (!isManifest && !isPreferredReadme) {
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
        if (isManifest) {
          manifest = text
        } else {
          readme = text
        }
        if (manifest != null && (!wantReadme || readme != null)) {
          settle()
          return
        }
        next()
      })

      stream.once('error', handleError)
    })

    extract.once('finish', () => settle(true))
    extract.once('error', handleError)
  })

  tarballStream.pipe(gunzip).pipe(extract)

  return promise
}

function readPackedEntry (file: fs.promises.FileHandle, tarballPath: TarballPath, targetIndex: number): Promise<string> {
  const extract = tar.extract()
  const gunzip = createGunzip()
  const tarballStream = file.createReadStream({ start: 0, autoClose: false })

  return new Promise((resolve, reject) => {
    let entryIndex = 0
    let settled = false

    function cleanup (): void {
      extract.destroy()
      gunzip.destroy()
      tarballStream.destroy()
    }

    function handleError (error: unknown): void {
      if (settled) return
      settled = true
      cleanup()
      reject(error)
    }

    tarballStream.once('error', handleError)
    gunzip.once('error', handleError)
    extract.once('error', handleError)
    extract.once('finish', () => {
      handleError(new PnpmError('PUBLISH_ARCHIVE_README_MISSING', `The archive ${tarballPath} no longer contains the selected README entry`))
    })

    extract.on('entry', (header, stream, next) => {
      if (entryIndex++ !== targetIndex) {
        stream.once('end', next)
        stream.resume()
        return
      }
      if (header.type !== 'file') {
        handleError(new PnpmError('PUBLISH_ARCHIVE_README_MISSING', `The archive ${tarballPath} no longer contains the selected README entry`))
        return
      }
      const chunks: Buffer[] = []
      stream.on('data', (chunk) => {
        chunks.push(chunk as Buffer)
      })
      stream.once('end', () => {
        if (settled) return
        settled = true
        cleanup()
        resolve(Buffer.concat(chunks).toString())
      })
      stream.once('error', handleError)
    })

    tarballStream.pipe(gunzip).pipe(extract)
  })
}

export class PublishArchiveMissingManifestError extends PnpmError {
  readonly tarballPath: string
  constructor (tarballPath: string) {
    super('PUBLISH_ARCHIVE_MISSING_MANIFEST', `The archive ${tarballPath} does not contain package/package.json`)
    this.tarballPath = tarballPath
  }
}
