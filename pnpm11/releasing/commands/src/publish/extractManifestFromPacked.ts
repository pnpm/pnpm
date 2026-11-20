import fs from 'node:fs'
import path from 'node:path'
import { inspect } from 'node:util'
import { createGunzip, type Gunzip } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import { type ExportedManifest, getReadmeRank, isPreferredReadme, type ReadmeCandidate } from '@pnpm/releasing.exportable-manifest'
import tar, { type Extract, type ExtractEvents, type Header } from 'tar-stream'

const MAX_BUFFERED_ENTRY_SIZE = 64 * 1024 * 1024

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
  const streams: PackedStreams = {
    extract: tar.extract(),
    gunzip: createGunzip(),
    tarballStream: fs.createReadStream(tarballPath),
  }
  const promise = collectPackedEntries(streams, { tarballPath, wantReadme })
  streams.tarballStream.pipe(streams.gunzip).pipe(streams.extract)
  return promise
}

interface PackedStreams {
  extract: Extract
  gunzip: Gunzip
  tarballStream: fs.ReadStream
}

interface PackedEntriesScan {
  manifest?: string
  readme?: PackedReadme
}

interface PackedEntriesScanContext {
  tarballPath: TarballPath
  scan: PackedEntriesScan
  wantReadme: boolean
  settle: () => void
  handleError: (error: unknown) => void
}

function collectPackedEntries (
  streams: PackedStreams,
  { tarballPath, wantReadme }: { tarballPath: TarballPath, wantReadme: boolean }
): Promise<PackedEntries> {
  let cleanedUp = false

  function cleanup (): void {
    if (cleanedUp) return
    cleanedUp = true

    streams.extract.destroy()
    streams.gunzip.destroy()
    streams.tarballStream.destroy()
  }

  return new Promise<PackedEntries>((resolve, reject) => {
    let settled = false
    const scan: PackedEntriesScan = {}

    function handleError (error: unknown): void {
      cleanup()
      reject(error)
    }

    function settle (): void {
      if (settled) return
      settled = true
      cleanup()
      if (scan.manifest == null) {
        reject(new PublishArchiveMissingManifestError(tarballPath))
        return
      }
      resolve({ manifest: scan.manifest, readme: scan.readme })
    }

    streams.tarballStream.once('error', handleError)
    streams.gunzip.once('error', handleError)

    const context: PackedEntriesScanContext = { tarballPath, scan, wantReadme, settle, handleError }
    streams.extract.on('entry', (header, stream, next) => {
      readPackedEntry({ header, stream, next }, context)
    })

    streams.extract.once('finish', settle)
    streams.extract.once('error', handleError)
  })
}

interface PackedEntry {
  header: Header
  stream: ExtractEvents['entry'][1]
  next: () => void
}

function readPackedEntry ({ header, stream, next }: PackedEntry, context: PackedEntriesScanContext): void {
  const normalizedPath = path.normalize(header.name).replaceAll('\\', '/')
  const isManifest = normalizedPath === 'package/package.json'
  const wantedReadme = isManifest ? undefined : pickWantedReadme(header, normalizedPath, context)

  if (!isManifest && wantedReadme == null) {
    stream.once('end', next)
    stream.resume()
    return
  }

  readBufferedPackedEntry({ header, stream, next }, context, wantedReadme)
}

function readBufferedPackedEntry (
  { header, stream, next }: PackedEntry,
  context: PackedEntriesScanContext,
  wantedReadme?: ReadmeCandidate
): void {
  if (!acceptBufferedEntrySize(header.size ?? 0, header.name, context)) return
  const chunks: Buffer[] = []
  let buffered = 0
  stream.on('data', (chunk) => {
    buffered += (chunk as Buffer).length
    if (!acceptBufferedEntrySize(buffered, header.name, context)) return
    chunks.push(chunk as Buffer)
  })

  stream.once('end', () => {
    const text = Buffer.concat(chunks).toString()
    if (wantedReadme == null) {
      context.scan.manifest = text
    } else {
      context.scan.readme = { ...wantedReadme, text }
    }
    // The manifest-only path stops at the manifest so the rest of the tarball isn't
    // decompressed. The README path scans on, so a later duplicate entry wins as on extraction.
    if (context.scan.manifest != null && !context.wantReadme) {
      context.settle()
      return
    }
    next()
  })

  stream.once('error', context.handleError)
}

function pickWantedReadme (header: Header, normalizedPath: string, context: PackedEntriesScanContext): ReadmeCandidate | undefined {
  const readmeCandidate = context.wantReadme && header.type === 'file'
    ? getRootReadmeCandidate(normalizedPath)
    : undefined
  return readmeCandidate != null && isPreferredReadme(readmeCandidate, context.scan.readme)
    ? readmeCandidate
    : undefined
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

function acceptBufferedEntrySize (size: number, filename: string, context: PackedEntriesScanContext): boolean {
  if (size <= MAX_BUFFERED_ENTRY_SIZE) return true
  context.handleError(new PnpmError(
    'PUBLISH_EXTRACT_MANIFEST_READ',
    `Failed to read the archive ${context.tarballPath}: tar entry ${inspect(filename, { colors: false })} is ${size} bytes, which exceeds the ${MAX_BUFFERED_ENTRY_SIZE}-byte buffered entry limit`
  ))
  return false
}
