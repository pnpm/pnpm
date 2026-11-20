import fs from 'node:fs'
import path from 'node:path'
import { createGzip } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import tar, { type Pack } from 'tar-stream'

export interface PackPkgOptions {
  destFile: string
  filesMap: Record<string, string>
  /** In-memory tar entries (name → contents) with no file on disk, e.g. the composed CHANGELOG.md. */
  injectedEntries?: Record<string, string>
  modulesDir: string
  packGzipLevel?: number
  bins: string[]
  manifest: ExportedManifest
}

export async function packPkg (opts: PackPkgOptions): Promise<void> {
  const mtime = new Date('1985-10-26T08:15:00.000Z')
  const pack = tar.pack()
  for (const entry of compressionOrderedEntries(opts.filesMap, opts.injectedEntries)) {
    addTarEntry(pack, entry, { ...opts, mtime })
  }
  const tarball = fs.createWriteStream(opts.destFile)
  pack.pipe(createGzip({ level: opts.packGzipLevel })).pipe(tarball)
  pack.finalize()
  return new Promise((resolve, reject) => {
    tarball.on('close', () => {
      resolve()
    }).on('error', reject)
  })
}

type PackedEntry =
  | { name: string, source: string }
  | { name: string, content: string }

interface TarEntryContext {
  bins: string[]
  manifest: ExportedManifest
  mtime: Date
}

function addTarEntry (pack: Pack, entry: PackedEntry, { bins, manifest, mtime }: TarEntryContext): void {
  if ('content' in entry) {
    pack.entry({ mode: 0o644, mtime, name: entry.name }, entry.content)
    return
  }
  if (!isManifestEntry(entry.name) && fs.lstatSync(entry.source).isSymbolicLink()) {
    addSymlinkEntry(pack, entry, mtime)
    return
  }
  const isExecutable = bins.some((bin) => path.relative(bin, entry.source) === '') || isFileExecutable(entry.source)
  const mode = isExecutable ? 0o755 : 0o644
  const content = isManifestEntry(entry.name)
    ? JSON.stringify(manifest, null, 2)
    : fs.readFileSync(entry.source)
  pack.entry({ mode, mtime, name: entry.name }, content)
}

/**
 * Packs a symlink only when its target stays inside the package, so the archive never links
 * outside of the directory it is extracted to.
 */
function addSymlinkEntry (pack: Pack, entry: { name: string, source: string }, mtime: Date): void {
  const linkname = readPackedLinkTarget(entry.source)
  const archiveDir = path.posix.dirname(entry.name)
  const posixTarget = linkname.replace(/\\/g, '/')
  if (path.posix.isAbsolute(posixTarget)) return
  const resolvedArchive = path.posix.normalize(path.posix.join(archiveDir, posixTarget))
  if (resolvedArchive !== 'package' && !resolvedArchive.startsWith('package/')) return
  pack.entry({ mode: 0o777, mtime, name: entry.name, type: 'symlink', linkname })
}

function readPackedLinkTarget (source: string): string {
  let linkname = fs.readlinkSync(source)
  if (path.isAbsolute(linkname)) {
    linkname = path.relative(path.dirname(source), linkname)
  }
  if (process.platform === 'win32') {
    linkname = linkname.replace(/\\/g, '/')
  }
  return linkname
}

/**
 * Every tar entry under the name it is packed as, ordered for compression.
 * `packlist()` already returns its own files that way; sorting here also
 * places the entries added afterwards, such as a workspace LICENSE or a
 * composed CHANGELOG.md.
 */
function compressionOrderedEntries (filesMap: Record<string, string>, injectedEntries?: Record<string, string>): PackedEntry[] {
  const entries: PackedEntry[] = [
    ...Object.entries(filesMap).map(([name, source]) => ({
      name: isManifestEntry(name) ? 'package/package.json' : name,
      source,
    })),
    ...Object.entries(injectedEntries ?? {}).map(([name, content]) => ({ name, content })),
  ]
  return entries.sort((entry1, entry2) => compareForCompression(entry1.name, entry2.name))
}

function compareForCompression (path1: string, path2: string): number {
  return path.extname(path1).toLowerCase().localeCompare(path.extname(path2).toLowerCase(), 'en') ||
    path.basename(path1).toLowerCase().localeCompare(path.basename(path2).toLowerCase(), 'en') ||
    path1.localeCompare(path2, 'en')
}

// True when a `package/<path>` tar key names the package manifest, which is
// packed as a single serialized `package/package.json` entry and reported as
// `package.json` in the contents listing regardless of the source file name.
export function isManifestEntry (name: string): boolean {
  return name === 'package/package.json' || name === 'package/package.json5' || name === 'package/package.yaml'
}

function isFileExecutable (file: string): boolean {
  try {
    return (fs.statSync(file).mode & 0o111) !== 0
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      return false
    }
    throw err
  }
}

export async function checkPackedBinsForCrlf (
  filesMap: Record<string, string>,
  bins: string[]
): Promise<void> {
  const binSet = new Set(bins.map((bin) => path.resolve(bin)))
  const packedBins = Object.entries(filesMap)
    .filter(([name, source]) => !isManifestEntry(name) && binSet.has(path.resolve(source)))
  await Promise.all(packedBins.map(async ([name, source]) => {
    if (await hasShebangWithCrlf(source)) {
      const relativePath = name.replace(/^package\//, '')
      throw new PnpmError(
        'BIN_CRLF',
        `The bin file "${relativePath}" has a shebang line ending with CRLF (\\r\\n).`,
        {
          hint: `CRLF line endings on the shebang line break execution on Unix systems (/usr/bin/env: 'node\\r': No such file or directory). Convert line endings of "${relativePath}" to LF (\\n).`,
        }
      )
    }
  }))
}

async function hasShebangWithCrlf (filePath: string): Promise<boolean> {
  let fileHandle: fs.promises.FileHandle | undefined
  try {
    fileHandle = await fs.promises.open(filePath, 'r')
    const buffer = Buffer.alloc(4096)
    const { bytesRead } = await fileHandle.read(buffer, 0, buffer.length, 0)
    return bufferHasShebangWithCrlf(buffer.subarray(0, bytesRead))
  } finally {
    await fileHandle?.close()
  }
}

function bufferHasShebangWithCrlf (buf: Buffer | Uint8Array): boolean {
  let start = 0
  if (buf.length >= 3 && buf[0] === 0xEF && buf[1] === 0xBB && buf[2] === 0xBF) {
    start = 3
  }
  if (buf.length < start + 2 || buf[start] !== 0x23 || buf[start + 1] !== 0x21) {
    return false
  }
  for (let byteIndex = start + 2; byteIndex < buf.length; byteIndex++) {
    if (buf[byteIndex] === 0x0D) {
      return true
    }
    if (buf[byteIndex] === 0x0A) {
      return false
    }
  }
  return false
}
